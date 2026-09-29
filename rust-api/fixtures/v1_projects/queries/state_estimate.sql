-- FX-Q-STATEEST — queryset SQL shapes, D-19 state/estimate domain.
-- Source: apps/api/pi_dash/api/views/state.py:47-60 (list), :170-183 (detail),
--   :231,278 (delete/patch direct gets); estimate.py:35-36,144-149,241-246
--   (querysets), :48,52,56,169-173,197-201 (direct filters).
-- Default managers: State.objects excludes group='triage' AND deleted rows
-- (models/state.py:79-84); Estimate/EstimatePoint use the soft-delete manager.
-- Bind params: %(slug)s, %(project)s (already UUID-rewritten), %(user)s.

-- S1 state list/detail (views/state.py:47-60,170-183; paginated on list).
SELECT s.* FROM states s
JOIN workspaces w ON w.id = s.workspace_id
JOIN projects p ON p.id = s.project_id
JOIN project_members vis
  ON (vis.project_id = s.project_id AND vis.member_id = %(user)s
      AND vis.is_active AND vis.deleted_at IS NULL)
WHERE w.slug = %(slug)s AND s.project_id = %(project)s
  AND s.group <> 'triage' AND s.deleted_at IS NULL
  AND p.archived_at IS NULL AND p.deleted_at IS NULL AND w.deleted_at IS NULL;
-- select_related project + workspace adds the JOINs above (already present
-- for filtering) and pulls p.*/w.* in one round trip. Archived projects hide
-- ALL their states; triage states never appear through objects.

-- S2 state delete/patch direct gets (views/state.py:231,278): both go through
-- State.objects, so triage rows and soft-deleted rows stay excluded (the
-- delete's explicit is_triage=False is redundant). Neither checks
-- project.archived_at — states of an ARCHIVED project can still be patched
-- and deleted while the list/detail querysets (S1) hide them.

-- E1 estimate queryset (views/estimate.py:35-36): no membership filter at the
-- queryset level (membership is enforced by ProjectEntityPermission only).
SELECT * FROM estimates e
JOIN workspaces w ON w.id = e.workspace_id
WHERE w.slug = %(slug)s AND e.project_id = %(project)s AND e.deleted_at IS NULL;
-- post checks Project then Workspace existence first (views/estimate.py:48-54),
-- then takes .first() and 409s when a row already exists (one estimate per
-- project is enforced in code, not by a DB constraint — race creates two).

-- E2 estimate-point list queryset (views/estimate.py:144-149).
SELECT ep.* FROM estimate_points ep
JOIN workspaces w ON w.id = ep.workspace_id
WHERE ep.estimate_id = %(estimate)s AND w.slug = %(slug)s
  AND ep.project_id = %(project)s AND ep.deleted_at IS NULL;
-- + select_related estimate, workspace, project (one round trip).
-- E3 detail queryset (views/estimate.py:241-246): same filters without the
-- select_related; patch/delete append AND ep.id = %(point)s then .first()
-- (missing -> 404 'Estimate point not found', views/estimate.py:265-267,288-289).
-- The list/get entry points ALSO verify the parent Estimate row first
-- (views/estimate.py:169-173,197-201): missing parent -> 404 'Estimate not found'
-- even when points exist.
