-- FX-A-Q-03 DefaultAnalytics (base.py:252-390) + ProjectStats (391-455).
-- Templates reconstructed from the Django ORM call sites (not EXPLAIN output).
-- Base: apps/api/pi_dash/.
--
-- (Q-03a) DefaultAnalytics base (base.py:255-258):
--   filters = issue_filters(request.GET, "GET")
--   base_issues = Issue.issue_objects.filter(workspace__slug=slug, **filters)
--   total_issues = base_issues.count()
SELECT COUNT(*) FROM "issues" <workspace joins>
 WHERE ("workspaces"."slug" = %s AND <filters>);
-- Seed row: total_issues = 3.
--
-- (Q-03b) classified totals (base.py:260-273):
--   state_groups = base_issues.annotate(state_group=F("state__group"))
--   total_issues_classified = state_groups.values("state_group")
--     .annotate(state_count=Count("state_group")).order_by("state_group")
--   open_issues_queryset = state_groups.filter(state__group__in=OPEN_STATE_GROUPS)
SELECT "states"."group" AS "state_group", COUNT("states"."group") AS "state_count"
  FROM "issues" INNER JOIN "states" ON ("issues"."state_id" = "states"."id")
 WHERE ("workspaces"."slug" = %s AND <filters>)
 GROUP BY "states"."group" ORDER BY "state_group" ASC;
-- Same shape filtered to OPEN_STATE_GROUPS (utils/constants) for open_issues /
-- open_issues_classified. Seed rows: total [{"backlog",2},{"completed",1}];
-- open_issues=2, open_classified=[{"backlog",2}].
--
-- (Q-03c) completed month-wise (base.py:275-282): current year only.
SELECT EXTRACT(MONTH FROM "issues"."completed_at") AS "month", COUNT(*) AS "count"
  FROM "issues" ... WHERE (... AND EXTRACT(YEAR FROM "completed_at") = %s)
 GROUP BY "month" ORDER BY "month" ASC;
-- Seed row: [{"month": <current month>, "count": 1}].
--
-- (Q-03d) most created top-5 (base.py:291-313): exclude created_by None;
--   avatar_url Case mirrors Q-01f with created_by__ prefix.
SELECT "users"."first_name" AS "created_by__first_name",
  "users"."last_name" AS "created_by__last_name",
  "users"."display_name" AS "created_by__display_name",
  "users"."id" AS "created_by__id", COUNT("issues"."id") AS "count",
  CASE WHEN "users"."avatar_asset" IS NOT NULL
       THEN CONCAT('/api/assets/v2/static/', "users"."avatar_asset", '/')
       WHEN "users"."avatar_asset" IS NULL THEN "users"."avatar"
       ELSE NULL END AS "created_by__avatar_url"
  FROM "issues" ... WHERE (... AND "issues"."created_by_id" IS NOT NULL)
 GROUP BY <user cols> ORDER BY "count" DESC LIMIT 5;
-- Seed row: one bucket {an_admin/User, count 3, avatar_url ""} — note the
-- empty-string avatar renders "" while the pending path renders NULL (Q-03f).
--
-- (Q-03e) most closed top-5 (base.py:322-345): completed_at non-null,
--   exclude assignees None, count AFTER the avatar Case annotation.
-- Seed row: [] (no assignees seeded).
--
-- (Q-03f) pending assignees (base.py:347-369): completed_at null, NO LIMIT.
-- Seed row: one NULL bucket {first/last/display/id NULL, count 2, avatar_url NULL}.
--
-- (Q-03g) estimates (base.py:371-372): open_estimate_sum =
--   open_issues_queryset.aggregate(sum=Sum("point"))["sum"]; same over base_issues.
--   PORT BUG: aggregates Sum("point") although the model join is estimate_point.
SELECT SUM("issues"."point") AS "sum" FROM "issues" ... WHERE <open|base scope>;
-- Seed rows: open_estimate_sum=8, total_estimate_sum=16. NULL (not 0) when no rows.
--
-- (Q-03h) ProjectStats (base.py:391-455):
--   fields=csv intersect {total_issues,completed_issues,total_members,total_cycles,total_modules};
--   empty/unknown → all five. projects = Project.objects.filter(workspace__slug);
--   optional id__in from project_ids csv. Each annotation is a correlated
--   subquery: Issue.issue_objects.filter(project_id=OuterRef(pk)).order_by()
--   .annotate(count=Func(F(id), function=Count)).values(count).
SELECT "projects"."id",
  (SELECT COUNT(U0."id") FROM "issues" U0 WHERE U0."project_id" = "projects"."id") AS "total_issues",
  (SELECT COUNT(U0."id") FROM "issues" U0 INNER JOIN "states" ...
    WHERE (U0."project_id" = "projects"."id" AND "states"."group" IN <CLOSED_STATE_GROUPS>)) AS "completed_issues",
  (SELECT COUNT(U0."id") FROM "cycles" U0 WHERE U0."project_id" = "projects"."id") AS "total_cycles",
  (SELECT COUNT(U0."id") FROM "modules" U0 WHERE U0."project_id" = "projects"."id") AS "total_modules",
  (SELECT COUNT(U0."id") FROM "project_members" U0 INNER JOIN "users" ...
    WHERE (U0."project_id" = "projects"."id" AND NOT "users"."is_bot" AND U0."is_active")) AS "total_members"
  FROM "projects" WHERE ("workspaces"."slug" = %s [AND "projects"."id" IN (...)])
-- Seed row: one project {total_issues 3, completed_issues 1, total_cycles 0,
-- total_modules 0, total_members 3}; fields=total_issues,completed_issues
-- returns only those keys plus id.
