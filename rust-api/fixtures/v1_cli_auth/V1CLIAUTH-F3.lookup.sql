-- V1CLIAUTH-F3 — lookup queries for DELETE /api/v1/runners/<id>/.
-- Source: apps/api/pi_dash/api/views/runner.py:45 (by-pk lookup),
--   apps/api/pi_dash/runner/services/permissions.py:70-77,126-137 (guard-facts read),
--   apps/api/pi_dash/runner/models.py:394 (pk), :478-480 (Meta: db_table + ordering).
-- Shapes follow the Django ORM compiler (quoted table/column names; no deleted
-- filter — there is no deleted_at column — and no JOIN on this path).
-- Bind param: %(runner_id)s / $1 the path UUID.

-- Q1 by-pk lookup: Runner.objects.filter(pk=runner_id).first() (views/runner.py:45).
-- .first() adds LIMIT 1; Meta.ordering ("-last_heartbeat_at", "-created_at")
-- (models.py:480) adds the ORDER BY. The ordering is semantically vacuous on a
-- pk lookup (at most one row matches), so the port uses the plain shape below.
SELECT "runner"."id", "runner"."owner_id", "runner"."workspace_id",
  "runner"."dev_machine_id", "runner"."pod_id", "runner"."name",
  "runner"."host_label", "runner"."provisioning", "runner"."visibility",
  "runner"."refresh_token_hash", "runner"."refresh_token_fingerprint",
  "runner"."refresh_token_generation", "runner"."previous_refresh_token_hash",
  "runner"."access_token_signing_key_version", "runner"."enrollment_token_hash",
  "runner"."enrollment_token_fingerprint", "runner"."enrolled_at",
  "runner"."capabilities", "runner"."status", "runner"."os", "runner"."arch",
  "runner"."runner_version", "runner"."dev_metadata", "runner"."protocol_version",
  "runner"."last_heartbeat_at", "runner"."free_worktrees", "runner"."created_at",
  "runner"."updated_at", "runner"."revoked_at", "runner"."revoked_reason"
FROM "runner"
WHERE "runner"."id" = %(runner_id)s
ORDER BY "runner"."last_heartbeat_at" DESC, "runner"."created_at" DESC
LIMIT 1;

-- Q1 port shape (same semantics; fetch_optional): the full 30-column row is
-- needed downstream — the guard reads (visibility, owner_id) off it and
-- delete_runner takes the row itself — so there is no narrow projection.
-- SELECT <30 cols> FROM runner WHERE id = $1

-- Q2 guard-facts read: NO second query. can_view_runner reads runner.visibility
-- + runner.owner_id (permissions.py:74-75) and can_manage_runner re-reads the
-- same attributes (permissions.py:135-136) off the Q1 row in Python.
-- The workspace-admin branch (permissions.py:137, is_workspace_admin over
-- workspace_members) is unreachable: it requires can_view True with a
-- non-PRIVATE visibility, and can_view True requires PRIVATE — so the
-- workspace_members query never fires on this path (see V1CLIAUTH-F4).
-- SELECT-less projection: (visibility, owner_id) FROM <Q1 row>.
