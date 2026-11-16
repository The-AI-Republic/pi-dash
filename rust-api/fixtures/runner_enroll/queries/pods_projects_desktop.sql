-- D13-F5 (part 4): pods, projects, desktop, machine-command reads.
-- Base: apps/api/pi_dash/. Method: hand-composed from cited ORM calls.

-- =====================================================================
-- P1. Pod list. pods.py:55-86. ?project= wins over ?workspace=.
-- =====================================================================
-- Project mode:
SELECT * FROM "projects" WHERE "id" = $1 LIMIT 1;
-- Miss -> 404 {"error": "project not found"} (SPACE, lowercase).
-- Non-member of project.workspace -> 403 {"error": "forbidden"}.
SELECT * FROM "pod"
WHERE "project_id" = $1 AND "deleted_at" IS NULL
ORDER BY "is_default" DESC, "created_at" ASC;
-- Workspace mode (no ?project=):
-- missing ?workspace= -> 400 {"error": "project or workspace is required"}.
-- non-member -> 403.
SELECT * FROM "pod"
WHERE "workspace_id" = $1 AND "deleted_at" IS NULL
ORDER BY "is_default" DESC, "created_at" ASC;
-- runner_count per row: SELECT COUNT(*) FROM "runner" WHERE "pod_id" = $p
-- (UNFILTERED — includes revoked; N+1, port shape not count).
-- project_identifier per row: needs pod.project.identifier — NOT
-- select_related'd (N+1 on projects; port shape not count).

-- =====================================================================
-- P2. Pod create. pods.py:88-130
-- =====================================================================
-- missing project or blank name -> 400 {"error": "project and name are required"}.
SELECT * FROM "projects" WHERE "id" = $1 LIMIT 1;
-- Miss -> 404 {"error": "project not found"}.
-- Non-ADMIN (is_workspace_admin, role>=20) -> 403 {"error": "workspace admin required"}.
-- NOTE create requires ADMIN; rename/delete require admin-OR-creator.
-- Bare-suffix convenience: name without "{identifier}_" prefix is re-prefixed
-- server-side; then validate_user_pod_name (see flows) -> 400 {"error": msg}.
INSERT INTO "pod"
    ("id", "workspace_id", "project_id", "name", "description",
     "created_by_id", "is_default", "deleted_at", "created_at", "updated_at")
VALUES ($1, $2, $3, $4, $5, $6, FALSE, NULL, now(), now());
-- description: request value or '' (None -> ''). Response: 201 serialized.
-- Unique violation (project,name active) is NOT caught -> 500 (port as-is;
-- no handler wraps the create).

-- =====================================================================
-- P3. Pod detail read. pods.py:139-145
-- =====================================================================
SELECT * FROM "pod" WHERE "id" = $1 AND "deleted_at" IS NULL LIMIT 1;
-- (default manager excludes tombstones.) Miss -> 404 "not found";
-- non-member -> 403 "forbidden".

-- =====================================================================
-- P4. Pod PATCH. pods.py:155-204. Manage gate: admin OR creator else 403.
-- =====================================================================
-- Rename: blank -> 400 {"error": "name cannot be empty"}; bare suffix
-- re-prefixed with pod.project.identifier (needs project row); validator ->
-- 400 {"error": msg}. Description: value or ''.
-- is_default promote (:192-197, inside tx): demote project siblings first:
-- BEGIN;
UPDATE "pod" SET "is_default" = FALSE
WHERE "project_id" = $1 AND "is_default" = TRUE AND "id" <> $2;
-- then pod.is_default=TRUE (applied in the final save below); COMMIT.
-- Demote (wants False while default): sets is_default=FALSE — leaving the
-- project with NO default (allowed! no guard). No-op when unchanged.
-- Final write (only when updates non-empty):
UPDATE "pod" SET <name|description|is_default...>, "updated_at" = now()
WHERE "id" = $2;
-- Response: 200 serialized (runner_count recomputed).

-- =====================================================================
-- P5. Pod DELETE guards + sweep (all inside tx, pod locked). pods.py:206-267
-- =====================================================================
-- Manage gate (admin-or-creator) BEFORE tx; then:
-- BEGIN;
SELECT * FROM "pod" WHERE "id" = $1 AND "deleted_at" IS NULL LIMIT 1 FOR UPDATE;
-- Miss (raced delete) -> 404 "not found".
-- Guard 1 — non-revoked runners (:228):
SELECT EXISTS(
    SELECT 1 FROM "runner" WHERE "pod_id" = $1 AND "status" <> 'revoked');
-- True -> 409 {"error": "pod has runners; move or revoke them first",
--              "code": "pod_has_runners"}.
-- Guard 2 — non-terminal runs (:236):
SELECT EXISTS(
    SELECT 1 FROM "agent_run" WHERE "pod_id" = $1 AND "status" IN
      ('queued','assigned','waiting_for_worktree','running','cancel_requested',
       'awaiting_approval','awaiting_reauth','paused_awaiting_input'));
-- True -> 409 {"error": "pod has non-terminal runs; cancel or wait",
--              "code": "pod_has_active_runs"}.
-- Guard 3 — default (:248): locked.is_default -> 409 {"error": "cannot delete
-- the project's default pod; promote another pod to default first",
-- "code": "default_pod_undeletable"}.
-- Guard ORDER is runners -> runs -> default (first hit wins).
UPDATE "pod" SET "deleted_at" = now(), "is_default" = FALSE, "updated_at" = now()
WHERE "id" = $1;
-- Issue sweep (:266):
UPDATE "issues" SET "assigned_pod_id" = NULL WHERE "assigned_pod_id" = $1;
-- COMMIT. Response: 204 empty.

-- =====================================================================
-- J1. Projects serialize. projects.py:31-77
-- =====================================================================
-- Pods per workspace, default-first then name:
SELECT "project_id", "is_default", "id", "name" FROM "pod"
WHERE "workspace_id" = $1 AND "deleted_at" IS NULL
ORDER BY "is_default" DESC, "name" ASC;
-- default_pod_id per project: FIRST row with is_default (dict-setdefault in
-- row order). pods[] embedded in the same row order.
SELECT * FROM "projects" WHERE "workspace_id" = $1 ORDER BY "identifier" ASC;
-- Response: [{id, identifier, name, description, is_default,
-- default_pod_id|null, pod_count, pods:[{id, name, is_default}]}].
-- All ids stringified; is_default bools.

-- =====================================================================
-- J2. Projects auth-mode dispatch. projects.py:97-124
-- =====================================================================
-- Mode 1: request.auth_runner set (Bearer mt_/JWT runner auth) ->
--   serialize(runner.workspace_id). No ?workspace honored.
-- Mode 2/3 (X-Api-Key / session): anonymous -> 401 {"error":
--   "authentication required"}. ?workspace= given: membership EXISTS probe
--   (WITHOUT is_active filter — raw WorkspaceMember.objects.filter(
--   workspace_id, member).exists()!) else 403 {"error": "forbidden"} ->
--   serialize(ws_filter). NOTE the missing is_active=True (every other
--   membership check in D-13 uses is_workspace_member WITH is_active) —
--   port as-is, listed quirk.
SELECT EXISTS(
    SELECT 1 FROM "workspace_members"
    WHERE "workspace_id" = $1 AND "member_id" = $2);
-- No filter: serialize EVERY membership workspace concatenated:
SELECT "workspace_id" FROM "workspace_members" WHERE "member_id" = $2;
-- (no is_active filter here either; no ORDER BY — DB order.)

-- =====================================================================
-- K1. Desktop enroll read-or-create (inside tx). desktop.py:107-130
-- =====================================================================
-- Guards BEFORE tx: workspace_slug blank -> 400; host_label blank -> 400;
-- version floor fail -> 409 desktop_update_required (see flows);
-- workspace miss/non-member -> 404 workspace_not_found (same body both).
-- BEGIN;
SELECT dm.* FROM "dev_machine" dm
WHERE dm."owner_id" = $1 AND dm."host_label" = $2
  AND dm."provisioning" = 'desktop_bundled' AND dm."revoked_at" IS NULL
  AND EXISTS (SELECT 1 FROM "machine_token" mt
              WHERE mt."dev_machine_id" = dm."id" AND mt."workspace_id" = $3)
ORDER BY dm."created_at" DESC LIMIT 1
FOR UPDATE;
-- Miss -> INSERT (provisioning desktop_bundled, label=host_label[:128],
-- last_seen_at=now). Hit -> UPDATE last_seen_at+updated_at (touch).
-- NOTE: the machine_tokens__workspace join does NOT filter revoked_at —
-- even a fully-revoked token row makes the machine reusable.

-- =====================================================================
-- K2. Desktop enroll token rotate (inside tx). desktop.py:132-149
-- =====================================================================
SELECT ... FOR UPDATE;  -- locked filter, then:
UPDATE "machine_token" SET "revoked_at" = now()
WHERE "dev_machine_id" = $1 AND "workspace_id" = $2 AND "revoked_at" IS NULL;
INSERT INTO "machine_token" (...) VALUES
  (..., label='desktop: ' || substr(host_label, 1, 88), is_service TRUE, ...);
-- COMMIT. Response: 201 {dev_machine_id, machine_token, workspace_slug,
-- managed_runner_enabled, graceful_stop_seconds}.

-- =====================================================================
-- K3. Desktop delete (sign-out). desktop.py:168-194
-- =====================================================================
SELECT "id" FROM "dev_machine"
WHERE "owner_id" = $1 AND "provisioning" = 'desktop_bundled'
  AND "revoked_at" IS NULL [AND "host_label" = $2];
-- host_label from body OR query (?), stripped [:255]; empty -> all machines.
-- No ids -> 204 immediately (no tx).
-- BEGIN;
UPDATE "machine_token" SET "revoked_at" = now()
WHERE "dev_machine_id" IN (...) AND "revoked_at" IS NULL;
UPDATE "runner" SET "status" = 'offline'
WHERE "dev_machine_id" IN (...) AND "provisioning" = 'desktop_bundled'
  AND "status" <> 'revoked';
-- COMMIT. 204. Runner rows SURVIVE (pod binding reused on next sign-in).

-- =====================================================================
-- C1. Machine-command reads. machine_commands.py:63-109,208-214,228-242
-- =====================================================================
-- _scoped_machine: machine by PK (NO lock) + M4 scope probes; miss/scope ->
-- 404 {"error": "not found"}; revoked machine -> 409 dev_machine_revoked.
-- Create validation: workspace required/membership (same as M-series);
-- project blank -> 400 {"error": "project is required"};
SELECT * FROM "workspaces" WHERE "id" = $1 LIMIT 1;   -- miss -> 404 workspace_not_found
SELECT EXISTS(SELECT 1 FROM "projects"
              WHERE "workspace_id" = $1 AND "identifier" = $2);
-- False -> 404 {"error": "project_not_found"}.
-- name: _RUNNER_NAME_RE else 400 invalid_runner_name + error_description.
-- agent default 'claude-code'; not in _VALID_AGENTS -> 400 invalid_agent.
-- Status read (web): M-series workspace gate + _scoped_machine, then Redis
-- get_command_result (see wire pins); binding check:
--   result None OR result.dev_machine_id NOT IN ('', None, str(machine_id))
--     -> 404 {"error": "unknown_request"} (cross-machine reads masquerade
--     as unknown). Else pop dev_machine_id, 200 {request_id, **result}.
-- Result write (daemon, mt_ auth): _auth_dev_machine predicate (see wire
-- pins); None -> 403 {"error": "dev_machine_mismatch"}; same binding
-- check vs str(machine.id) -> 404 unknown_request; status not in
-- {ok, error} -> 400 {"error": "invalid_status"}; write payload
-- {status, dev_machine_id, runner_id str, runner_name str[:128],
--  error str[:512], reported_at iso} with TTL 900; 204.
