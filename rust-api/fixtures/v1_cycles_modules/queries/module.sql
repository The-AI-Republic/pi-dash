-- queries/module.sql — FX-CYCMOD-05. Transcribed from the get_queryset chains cited per block.
-- Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52 (zero diff to HEAD, verified 2026-09-29).

-- M1 ModuleList queryset (views/module.py:85-171) + GET archived filter (views/module.py:272).
-- Module.objects.filter(project_id, workspace__slug)  (:87-88) — NOTE: no member-active filter here
-- (unlike cycles); permission enforced by ProjectEntityPermission only.
--   .select_related(project, workspace, lead) (:89-91)
--   .prefetch_related(members) (:92)
--   .prefetch_related(Prefetch(link_module, ModuleLink.objects.select_related(module, created_by))) (:93-98)
--   .annotate(total/completed/cancelled/started/unstarted/backlog_issues=Count(issue_module...) FILTER live, distinct=True) (:99-169)
--   .order_by(<order_by or -created_at>) (:170) — no .distinct() (M2 detail also omits it; cycle querysets keep .distinct())
SELECT "modules".*, COUNT(DISTINCT "mi"."id") FILTER (
    WHERE "i"."archived_at" IS NULL AND "i"."is_draft" = FALSE AND "mi"."deleted_at" IS NULL
  ) AS "total_issues",
  COUNT(DISTINCT "mi"."issue__state__group") FILTER (
    WHERE "mi"."issue__state__group" = '<group>' AND "i"."archived_at" IS NULL
      AND "i"."is_draft" = FALSE AND "mi"."deleted_at" IS NULL
  ) AS "<group>_issues"  -- repeated per group
  FROM "modules"
  LEFT OUTER JOIN "module_issues" "mi" ON ("modules"."id" = "mi"."module_id")
  LEFT OUTER JOIN "issues" "i" ON ("mi"."issue_id" = "i"."id")
  WHERE ("modules"."project_id" = %(project)s AND "modules"."workspace_id" = %(workspace)s
    AND "modules"."deleted_at" IS NULL AND "modules"."archived_at" IS NULL)  -- GET adds archived_at IS NULL (:272)
  GROUP BY "modules"."id" ORDER BY "modules"."created_at" DESC;
-- members + link_module arrive via separate prefetch queries (no SQL fanout).

-- M2 ModuleDetail queryset (views/module.py:288-374): identical to M1; GET applies
-- .filter(archived_at__isnull=True).get(pk) (views/module.py:473).

-- M3 Archived-modules list (views/module.py:895-...): same as M1 with archived_at__isnull=False (:899);
-- NO estimate annotations (unlike archived cycles Q3) — PORT the asymmetry.

-- M4 ModuleIssue list queryset (views/module.py:545-569) vs GET inline queryset (views/module.py:601-634).
-- get_queryset: ModuleIssue.objects.annotate(sub_issues_count=Issue.issue_objects.filter(parent=OuterRef(issue))...Count) (:547-552)
--   .filter(workspace__slug, project_id, module_id, member-active, project__archived_at IS NULL) (:553-560)
--   .select_related(project, workspace, module, issue+state+project) (:561-564)
--   .prefetch_related(issue__assignees, issue__labels, module__members) (:565-566)
--   .order_by(-created_at).distinct() (:567-568)
-- GET (:601-634): Issue.issue_objects.filter(issue_module__module_id, issue_module__deleted_at IS NULL) (:602)
--   .annotate(sub_issues_count, bridge_id=F(issue_module__id)) (:603-609)
--   .filter(project_id, workspace__slug) + select/prefetch (:610-617) .order_by(created_at) (:618)
--   .annotate(link_count, attachment_count) (:619-633) — same shape as cycle Q4.
SELECT "issues".*,
  (SELECT COUNT(U0."id") FROM "issues" U0 WHERE (U0."deleted_at" IS NULL AND U0."parent_id" = "issues"."id")) AS "sub_issues_count",
  "module_issues"."id" AS "bridge_id",
  (SELECT COUNT(U0."id") FROM "issue_links" U0 WHERE U0."issue_id" = "issues"."id") AS "link_count",
  (SELECT COUNT(U0."id") FROM "file_assets" U0 WHERE (U0."issue_id" = "issues"."id" AND U0."entity_type" = 'ISSUE_ATTACHMENT')) AS "attachment_count"
  FROM "issues" INNER JOIN "module_issues" ON ("issues"."id" = "module_issues"."issue_id")
  WHERE ("module_issues"."module_id" = %(module)s AND "module_issues"."deleted_at" IS NULL
    AND "issues"."project_id" = %(project)s AND "issues"."workspace_id" = %(workspace)s
    AND "issues"."deleted_at" IS NULL) ORDER BY "issues"."created_at" ASC;

-- M5 ModuleIssue detail (views/module.py:751-775 queryset; 800-849 get; 864-888 delete):
-- queryset mirrors M4 get_queryset; GET re-queries Issue.issue_objects.filter(issue_module__module_id,
-- issue_module__deleted_at IS NULL, pk=issue_id) + same annotations, paginated (:807-849).
-- DELETE: ModuleIssue.objects.get(workspace__slug, project_id, module_id, issue_id) then .delete() (:870-878).
