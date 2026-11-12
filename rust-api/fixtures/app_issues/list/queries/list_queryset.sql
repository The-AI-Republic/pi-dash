-- queries/list_queryset.sql
-- Issue-list family base SQL shapes (pilot 2, PIDASHCONV-11).
-- Templates reconstructed from the Django ORM call sites (not EXPLAIN
-- output): table/column names follow db/models/issue.py; subquery shapes
-- follow base.py get_queryset/apply_annotations and the grouper at
-- grouper.py:30. Verified behaviourally: the list subset (14 tests) passes
-- byte-identical against both backends through the proxy.
--
-- (1) Flat issues/list/ base (IssueListEndpoint.get, base.py:~98):
-- Issue.issue_objects.filter(workspace__slug=slug, project_id=project_id,
-- pk__in=issue_ids). issue_objects = IssueManager() (db/models/issue.py:229).
SELECT "issues"."id", "issues"."name", "issues"."state_id",
  "issues"."sort_order", "issues"."completed_at", "issues"."estimate_point",
  "issues"."priority", "issues"."start_date", "issues"."target_date",
  "issues"."sequence_id", "issues"."project_id", "issues"."parent_id",
  "issues"."cycle_id",
  (SELECT COALESCE(ARRAY_AGG(U0."assignee_id"), '{}')
     FROM "issue_assignees" U0
    WHERE (U0."deleted_at" IS NULL AND U0."issue_id" = ("issues"."id"))) AS "assignee_ids",
  (SELECT COALESCE(ARRAY_AGG(U0."label_id"), '{}')
     FROM "issue_labels" U0
    WHERE (U0."deleted_at" IS NULL AND U0."issue_id" = ("issues"."id"))) AS "label_ids",
  (SELECT COALESCE(ARRAY_AGG(U0."module_id"), '{}')
     FROM "issue_modules" U0
    WHERE (U0."deleted_at" IS NULL AND U0."archived_at" IS NULL
           AND U0."issue_id" = ("issues"."id"))) AS "module_ids",
  (SELECT COUNT(U0."id") FROM "issues" U0
    WHERE (U0."deleted_at" IS NULL AND U0."parent_id" = ("issues"."id"))) AS "sub_issues_count",
  "issues"."created_at", "issues"."updated_at",
  "issues"."created_by_id", "issues"."updated_by_id",
  "issues"."attachment_count", "issues"."link_count",
  "issues"."is_draft", "issues"."archived_at", "issues"."deleted_at"
  FROM "issues"
  INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
  INNER JOIN "workspaces" ON ("projects"."workspace_id" = "workspaces"."id")
 WHERE ("workspaces"."slug" = %s AND "issues"."project_id" = %s
        AND "issues"."id" IN (%s, ...));
-- NOTE the assignee subquery carries NO active-member filter and the module
-- subquery carries NO module.deleted_at check on the Django side; the Rust
-- port adds m.deleted_at IS NULL (DIVERGENCE, recorded for fix).
-- Coalesce->[] on both sides: empty arrays render [] not null.
--
-- (2) Paginated issues/ base (IssueViewSet.get_queryset, base.py:210-216):
-- same SELECT list as (1) plus state__group (issue_on_results appends it
-- even when group_by is falsy), with .distinct() and the manager's
-- draft/archived exclusion:
SELECT DISTINCT <same list as (1) plus "states"."group" AS "state__group">
  FROM "issues"
  INNER JOIN "states" ON ("issues"."state_id" = "states"."id")
  <same joins as (1)>
 WHERE ("issues"."project_id" = %s AND "workspaces"."slug" = %s
        AND "issues"."is_draft" = false AND "issues"."archived_at" IS NULL)
 ORDER BY "issues"."created_at" DESC;
-- Seed result: 3 rows (I1 parent, I2 child, I3 done); I4 draft + I5
-- archived excluded (test_crud_list_paginated_shape).
--
-- (3) issues-detail/ permission gate (IssueDetailEndpoint.get, base.py:1044-~1071):
-- literal Exists over the member row — three OR branches, no simplification:
SELECT <detail 25-key list, arrays via unguarded prefetch reads>
  FROM "issues" WHERE ("workspaces"."slug" = %s AND "issues"."project_id" = %s
  AND EXISTS (SELECT U0."id" FROM "issues" U0
    INNER JOIN "projects" ... INNER JOIN "project_members" ...
    WHERE (U0."id" = ("issues"."id")
      AND ((member = %s AND is_active AND role > 5)
        OR (member = %s AND is_active AND role = 5 AND guest_view_all_features)
        OR (member = %s AND is_active AND role = 5
            AND NOT guest_view_all_features AND created_by_id = %s)))));
-- Detail arrays are prefetch reads on plain managers: NO
-- deleted/archived/active guards at all (no custom objects on through
-- models/BaseModel) — ported as unguarded_arrays.
--
-- (4) deleted-issues/ (DeletedIssuesListViewSet.get, base.py:816-824):
-- Issue.all_objects (NO manager exclusion) + archived-or-deleted OR:
SELECT "issues"."id" FROM "issues"
  <same workspace joins as (1)>
 WHERE ("workspaces"."slug" = %s AND "issues"."project_id" = %s
        AND ("issues"."archived_at" IS NOT NULL OR "issues"."deleted_at" IS NOT NULL)
        [AND "issues"."updated_at" > %s]);
-- Body is the bare id array; no envelope, no pagination.
--
-- (5) Ordering (order_issue_queryset + paginator re-ordering):
-- ORDER BY <key> [DESC] applied AFTER annotations (aliases orderable);
-- grouped windows re-order rows after bucketing. Default -created_at.
