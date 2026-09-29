-- FX-A-Q-01 AnalyticsEndpoint aggregations (app/views/analytic/base.py:38-176).
-- FX-A-Q-02 AnalyticViewViewset queryset (base.py:177-189) + SavedAnalytic (190-222) + ExportAnalytics (223-251).
-- Templates reconstructed from the Django ORM call sites (not EXPLAIN
-- output), same convention as fixtures/app_issues/list/queries/list_queryset.sql.
-- Table names follow db/models (analytic_views, exporters, issues, states).
-- Base: apps/api/pi_dash/.
--
-- (Q-01a) AnalyticsEndpoint base queryset (base.py:60-66):
--   filters = issue_filters(request.GET, "GET")
--   queryset = Issue.issue_objects.filter(workspace__slug=slug, **filters)
--   total_issues = queryset.count()
SELECT COUNT(*) FROM "issues"
  INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
  INNER JOIN "workspaces" ON ("projects"."workspace_id" = "workspaces"."id")
 WHERE ("workspaces"."slug" = %s AND <issue_filters GET predicates>);
-- Seed row: total = 3 (an-ws world).
--
-- (Q-01b) build_graph_plot issue_count branch (utils/analytics_plot.py:96-107):
--   x_axis annotated as dimension (F(x_axis), or year||'-'||month Concat for
--   created_at/start_date/target_date/completed_at via annotate_with_monthly_dimension);
--   NULL dimensions excluded only for date axes (:85-86).
--   Non-date: annotate is_null=Case(When(dimension__isnull → 'None') default 'not_null'),
--   dimension_ex=Coalesce(dimension,'null'); values(dimension[, segment]);
--   annotate(count=Count('*')).order_by(dimension).
SELECT "dimension", COUNT(*) AS "count" FROM (
  SELECT <x_axis expr> AS "dimension"[, <segment expr> AS "segment"]
    FROM "issues" <joins per x_axis> WHERE ("workspaces"."slug" = %s AND <filters>)
) GROUP BY "dimension"[, "segment"] ORDER BY "dimension" ASC;
-- Python regroups rows by str(dimension) and applies sort_data: priority axes
-- sort low,medium,high,urgent,none (missing keys dropped); all other axes sort
-- with 'none' last (:64-70). Seed row: {"high":[{"dimension":"high","count":1}],
-- "medium":[...], "urgent":[...]}.
--
-- (Q-01c) build_graph_plot estimate branch (analytics_plot.py:110-115):
--   annotate(estimate=Sum(Cast(estimate_point__value AS float))).order_by(x_axis).
SELECT "dimension", SUM(CAST("estimate_points"."value" AS DOUBLE PRECISION)) AS "estimate"
  FROM "issues" LEFT OUTER JOIN "estimate_points" ...
 GROUP BY "dimension" ORDER BY <x_axis> ASC;
-- Seed row: keys {high,medium,urgent}, one bucket each with {dimension, estimate}.
--
-- (Q-01d) state_details (base.py:72-78): only when x_axis or segment == state_id.
--   Issue.issue_objects.filter(workspace__slug, **filters)
--   .distinct("state_id").order_by("state_id")
--   .values("state_id","state__name","state__color")
SELECT DISTINCT ON ("issues"."state_id") "issues"."state_id",
  "states"."name" AS "state__name", "states"."color" AS "state__color"
  FROM "issues" INNER JOIN "states" ON ("issues"."state_id" = "states"."id")
 WHERE ("workspaces"."slug" = %s AND <filters>) ORDER BY "issues"."state_id" ASC;
--
-- (Q-01e) label_details (base.py:81-92): NOTE uses plain Issue.objects
--   (not issue_objects — no manager exclusion) + labels__id__isnull=False
--   + label_issue__deleted_at__isnull=True.
SELECT DISTINCT ON ("labels"."id") "labels"."id" AS "labels__id",
  "labels"."color" AS "labels__color", "labels"."name" AS "labels__name"
  FROM "issues" ... WHERE ("workspaces"."slug" = %s AND <filters>
  AND "labels"."id" IS NOT NULL AND "label_issue"."deleted_at" IS NULL)
 ORDER BY "labels"."id" ASC;
--
-- (Q-01f) assignee_details (base.py:94-131): avatar OR avatar_asset non-null,
--   avatar_url = Case(When(avatar_asset non-null → Concat('/api/assets/v2/static/', avatar_asset, '/'))
--   When(avatar_asset null → avatar) default NULL).
SELECT DISTINCT ON ("users"."id") "users"."id" AS "assignees__id",
  CASE WHEN "users"."avatar_asset" IS NOT NULL
       THEN CONCAT('/api/assets/v2/static/', "users"."avatar_asset", '/')
       WHEN "users"."avatar_asset" IS NULL THEN "users"."avatar"
       ELSE NULL END AS "assignees__avatar_url",
  "users"."display_name" AS "assignees__display_name",
  "users"."first_name" AS "assignees__first_name",
  "users"."last_name" AS "assignees__last_name"
  FROM "issues" ... WHERE ("workspaces"."slug" = %s AND <filters>
  AND ("users"."avatar" IS NOT NULL OR "users"."avatar_asset" IS NOT NULL))
 ORDER BY "users"."id" ASC;
--
-- (Q-01g) cycle_details (base.py:133-145): issue_cycle__cycle_id non-null
--   + issue_cycle__deleted_at non-null guard.
SELECT DISTINCT ON ("cycles"."id") "cycles"."id" AS "issue_cycle__cycle_id",
  "cycles"."name" AS "issue_cycle__cycle__name" ... WHERE (...
  AND "cycle_issues"."cycle_id" IS NOT NULL AND "cycle_issues"."deleted_at" IS NULL);
--
-- (Q-01h) module_details (base.py:147-159): same shape via issue_module.
SELECT DISTINCT ON ("modules"."id") "modules"."id" AS "issue_module__module_id",
  "modules"."name" AS "issue_module__module__name" ... WHERE (...
  AND "module_issues"."module_id" IS NOT NULL AND "module_issues"."deleted_at" IS NULL);
--
-- (Q-02a) AnalyticViewViewset.get_queryset (base.py:186-187):
--   super().get_queryset().filter(workspace__slug=slug); perform_create (:182-184)
--   looks up Workspace by slug then serializer.save(workspace_id=...).
--   Default viewset ordering applies (Meta.ordering = -created_at).
SELECT * FROM "analytic_views"
 WHERE ("analytic_views"."workspace_id" IN
   (SELECT "workspaces"."id" FROM "workspaces" WHERE "workspaces"."slug" = %s))
 ORDER BY "analytic_views"."created_at" DESC;
--
-- (Q-02b) SavedAnalyticEndpoint (base.py:190-220):
--   AnalyticView.objects.get(pk=analytic_id, workspace__slug=slug) — plain
--   manager; filter = analytic_view.query (stored JSON used VERBATIM as ORM
--   kwargs, no re-validation); queryset = Issue.issue_objects.filter(**filter);
--   x_axis/y_axis come from query_dict (NOT the request); segment still from
--   request.GET; distribution/total reuse Q-01b/Q-01a shapes.
-- Seed row: AV1 query={"workspace__slug":"an-ws"} total=3, priority buckets as Q-01b.
--
-- (Q-02c) ExportAnalyticsEndpoint (base.py:223-249): NO queryset — validates
--   x_axis/y_axis/segment (same 400 bodies as Q-01), then
--   analytic_export_task.delay(email, data, slug) and returns the emailed-to message.
