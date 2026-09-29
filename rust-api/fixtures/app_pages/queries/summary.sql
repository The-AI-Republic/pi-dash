-- queries/summary.sql
-- PageViewSet.summary aggregate record: SQL + rows.
-- Source: app/views/page/base.py:421-469. Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.
--
-- R1 queryset (:422-438): same membership/active/not-archived guards as
--   get_queryset BUT WITHOUT parent__isnull (children counted), WITHOUT
--   owner-or-public? No — WITH Q(owned_by=user)|Q(access=0) (:430),
--   WITH project Exists + filter(project=True) (:431-437), .distinct() (:437).
-- R2 guest scoping (:441-451): role == GUEST(5, app/permissions/base.py:16)
--   AND active AND NOT project.guest_view_all_features
--   -> queryset.filter(owned_by=user) (:451).
-- R3 aggregates (:453-467):
--   public_pages  = COUNT(CASE WHEN access=0(PUBLIC_ACCESS) AND archived_at IS NULL THEN 1 END)
--   private_pages = COUNT(CASE WHEN access=1(PRIVATE_ACCESS) AND archived_at IS NULL THEN 1 END)
--   archived_pages= COUNT(CASE WHEN archived_at IS NOT NULL THEN 1 END)
-- OVERLAP behaviour (record as observed — PORT): archived public/private pages
--   are ALSO counted in archived_pages (no mutual exclusion); a non-archived
--   page lands in exactly one of public/private by its access value.
--
-- Representative SQL:
SELECT
  COUNT(CASE WHEN pages.access = 0 AND pages.archived_at IS NULL THEN 1 END) AS public_pages,
  COUNT(CASE WHEN pages.access = 1 AND pages.archived_at IS NULL THEN 1 END) AS private_pages,
  COUNT(CASE WHEN pages.archived_at IS NOT NULL THEN 1 END) AS archived_pages
FROM (SELECT DISTINCT pages.id, pages.access, pages.archived_at FROM pages
  INNER JOIN project_pages ON (pages.id = project_pages.page_id)
  INNER JOIN projects ON (project_pages.project_id = projects.id)
  INNER JOIN project_members pm
    ON (projects.id = pm.project_id AND pm.member_id = :user AND pm.is_active
      AND projects.archived_at IS NULL)
  WHERE pages.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
    AND (pages.owned_by_id = :user OR pages.access = 0)
    AND EXISTS(SELECT 1 FROM project_pages ppf
      WHERE ppf.page_id = pages.id AND ppf.project_id = :project_id)
    -- guest scoping (:451) appended only for guests without view-all:
    -- AND pages.owned_by_id = :user
) scoped;
