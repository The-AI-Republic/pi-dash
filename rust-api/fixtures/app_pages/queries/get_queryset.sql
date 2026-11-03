-- queries/get_queryset.sql
-- PageViewSet.get_queryset record: SQL shape + result rows.
-- Source: app/views/page/base.py:81-127. Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.
--
-- R1 base filters, in order (:88-98):
--   workspace__slug = :slug                                  (:91)
--   projects__project_projectmember__member = :user
--     AND projects__project_projectmember__is_active = true
--     AND projects__archived_at IS NULL                      (:92-96, JOIN via projects M2M -> project_pages -> project -> projectmember)
--   parent_id IS NULL (top-level only)                       (:97)
--   (owned_by_id = :user OR access = 0) (owner-or-public)    (:98)
-- R2 annotations:
--   is_favorite = EXISTS(SELECT 1 FROM user_favorites
--     WHERE user_id = :user AND entity_type = 'page'
--       AND entity_identifier = pages.id                      -- OuterRef('pk') (:82-87)
--       AND workspace__slug = :slug)                         (:102)
-- R3 ordering BUG (:103 then :105 — the second order_by REPLACES the first in
--   Django; the request's ?order_by= value is silently discarded; PORT):
--   .order_by(request.GET.get('order_by', '-created_at'))    (:103, dead)
--   .order_by('-is_favorite', '-created_at')                 (:105, effective)
-- R4 annotations (:106-124):
--   project = EXISTS(SELECT 1 FROM project_pages
--     WHERE page_id = pages.id AND project_id = :project_id) (:107-110)
--   label_ids = COALESCE(ARRAY_AGG(DISTINCT page_labels.label_id)
--     FILTER (WHERE page_labels.label_id IS NOT NULL), '{}') (:112-119)
--   project_ids = COALESCE(ARRAY_AGG(DISTINCT projects.id)
--     FILTER (WHERE NOT (projects.id = TRUE)), '{}')         (:120-123)
--     -- BUG-PORT (:121): filter=~Q(projects__id=True) compares a UUID col
--     -- to boolean TRUE; on Postgres this is a type error-or-always-true
--     -- no-op. The Coalesce-[] empty guard is what actually renders [].
-- R5 tail: .filter(project = TRUE) (:125) then .distinct() (:126),
--   then filter_queryset() (search ?search= on name via search_fields (:79)).
--
-- Representative SQL (placeholders :slug, :user, :project_id):
SELECT DISTINCT pages.*,
  EXISTS(SELECT 1 FROM user_favorites
    WHERE user_favorites.user_id = :user
      AND user_favorites.entity_type = 'page'
      AND user_favorites.entity_identifier = pages.id
      AND user_favorites.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  ) AS is_favorite,
  EXISTS(SELECT 1 FROM project_pages
    WHERE project_pages.page_id = pages.id
      AND project_pages.project_id = :project_id
  ) AS project,
  COALESCE((SELECT ARRAY_AGG(DISTINCT page_labels.label_id)
    FROM page_labels WHERE page_labels.page_id = pages.id
      AND page_labels.label_id IS NOT NULL), '{}') AS label_ids,
  COALESCE((SELECT ARRAY_AGG(DISTINCT project_pages_2.project_id)
    FROM project_pages project_pages_2 WHERE project_pages_2.page_id = pages.id), '{}') AS project_ids
FROM pages
  INNER JOIN project_pages ON (pages.id = project_pages.page_id)
  INNER JOIN projects ON (project_pages.project_id = projects.id)
  INNER JOIN project_members projectmember
    ON (projects.id = projectmember.project_id
      AND projectmember.member_id = :user AND projectmember.is_active
      AND projects.archived_at IS NULL)
WHERE pages.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND pages.parent_id IS NULL
  AND (pages.owned_by_id = :user OR pages.access = 0)
  AND EXISTS(SELECT 1 FROM project_pages ppf
    WHERE ppf.page_id = pages.id AND ppf.project_id = :project_id)
ORDER BY is_favorite DESC, pages.created_at DESC;
