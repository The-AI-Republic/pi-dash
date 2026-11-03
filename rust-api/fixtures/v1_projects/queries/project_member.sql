-- FX-Q-PROJMEM — queryset SQL shapes, D-19 project/member/invite/user domain.
-- Source: apps/api/pi_dash/api/views/project.py:82-140 (list), :297-355 (detail),
--   :169-187 (GET sort_order+prefetch); member.py:82,135-141,189-192; invite.py:36-40; user.py:40.
-- Shapes follow the Django ORM compiler (table names from Meta.db_table; the
-- default manager adds "deleted_at" IS NULL on every table incl. subqueries).
-- Bind params: %(slug)s workspace slug, %(user)s request user id.

-- Q1 project list/detail base (detail adds select_related workspace/owner/
-- default_assignee/project_lead; list selects project_lead only).
SELECT p.*,
  EXISTS(SELECT 1 FROM project_members pm
         WHERE pm.member_id = %(user)s AND pm.project_id = p.id
           AND pm.workspace_id = w.id AND pm.is_active AND pm.deleted_at IS NULL
        ) AS is_member,
  (SELECT COUNT(*) FROM project_members pm
    JOIN users u ON u.id = pm.member_id
    WHERE pm.project_id = p.id AND u.is_bot = FALSE
      AND pm.is_active AND pm.deleted_at IS NULL) AS total_members,
  (SELECT COUNT(*) FROM cycles c
    WHERE c.project_id = p.id AND c.deleted_at IS NULL) AS total_cycles,
  (SELECT COUNT(*) FROM modules m
    WHERE m.project_id = p.id AND m.deleted_at IS NULL) AS total_modules,
  (SELECT pm.role FROM project_members pm
    WHERE pm.project_id = p.id AND pm.member_id = %(user)s
      AND pm.is_active AND pm.deleted_at IS NULL) AS member_role,
  EXISTS(SELECT 1 FROM deploy_boards d
         WHERE d.project_id = p.id AND d.workspace_id = w.id
           AND d.deleted_at IS NULL) AS is_deployed
FROM projects p
JOIN workspaces w ON w.id = p.workspace_id
LEFT OUTER JOIN project_members vis
  ON (vis.project_id = p.id AND vis.member_id = %(user)s AND vis.is_active
      AND vis.deleted_at IS NULL)
WHERE w.slug = %(slug)s AND p.deleted_at IS NULL
  AND (vis.id IS NOT NULL OR p.network = 2)
GROUP BY p.id, w.id
-- ORDER BY: list base uses kwargs order_by default -created_at, but GET
-- re-orders by request.GET order_by default 'sort_order' (views/project.py:186);
-- sort_order itself is a second annotation (Q2). Detail base keeps -created_at.

-- Q2 list-GET sort_order annotation + member prefetch (views/project.py:169-187).
-- SELECT ..., (SELECT pm.sort_order FROM project_members pm
--               WHERE pm.member_id = %(user)s AND pm.project_id = p.id
--                 AND pm.workspace_id = w.id AND pm.is_active
--                 AND pm.deleted_at IS NULL) AS sort_order
-- ORDER BY sort_order  (NULLS sort per Postgres default for the direction)
-- Prefetch: SELECT ... FROM project_members
--   WHERE workspace_id = %(ws)s AND is_active AND deleted_at IS NULL
--   (+ select_related member) for the projects on the page.

-- Q3 workspace members (views/member.py:76-82; 400 when the slug is unknown).
-- SELECT 1 FROM workspaces WHERE slug = %(slug)s;  -- exists() check
SELECT wm.*, u.* FROM workspace_members wm
JOIN users u ON u.id = wm.member_id
WHERE wm.workspace_id = %(ws)s AND wm.deleted_at IS NULL AND u.deleted_at IS NULL;

-- Q4 project members ids then users (views/member.py:135-141).
SELECT pm.member_id FROM project_members pm
JOIN workspaces w ON w.id = pm.workspace_id
WHERE pm.project_id = %(project)s AND w.slug = %(slug)s
  AND pm.deleted_at IS NULL;
SELECT u.* FROM users u WHERE u.id IN (...) AND u.deleted_at IS NULL;

-- Q5 project member detail (views/member.py:189-192,204,219).
SELECT * FROM project_members pm JOIN workspaces w ON w.id = pm.workspace_id
WHERE pm.project_id = %(project)s AND w.slug = %(slug)s AND pm.id = %(pk)s
  AND pm.deleted_at IS NULL;  -- .get(): DoesNotExist -> base handle_exception 404
SELECT * FROM users WHERE id = %(member)s AND deleted_at IS NULL;

-- Q6 invite queryset/object (views/invite.py:36-40; BaseViewSet pagination/filter backends apply).
SELECT * FROM workspace_member_invites i
JOIN workspaces w ON w.id = i.workspace_id
WHERE w.slug = %(slug)s AND i.deleted_at IS NULL;
-- get_object adds AND i.id = %(pk)s.

-- Q7 current user (views/user.py:40): no query — serializes request.user from auth.
