-- D13-F5 (part 3): machine + runner web reads/writes.
-- Base: apps/api/pi_dash/. Method: hand-composed from cited ORM calls.

-- =====================================================================
-- M1. Control-online subquery. runners.py:52-59
-- =====================================================================
-- Exists(MachineSession where dev_machine=OuterRef(pk), active,
--        last_seen_at >= now() - 90s):
EXISTS(
    SELECT 1 FROM "machine_session" ms
    WHERE ms."dev_machine_id" = "dev_machine"."id"
      AND ms."revoked_at" IS NULL
      AND ms."last_seen_at" >= (now() - interval '90 seconds')
) AS "control_online"
-- The cutoff timestamp is computed ONCE per request (timezone.now() at
-- annotation time), not per row.

-- =====================================================================
-- M2. Machine list scoping probes. runners.py:83-101
-- =====================================================================
SELECT DISTINCT "dev_machine_id" FROM "runner"
WHERE "workspace_id" = $1 AND "owner_id" = $2
  AND "visibility" = 0 AND "dev_machine_id" IS NOT NULL;
SELECT DISTINCT "dev_machine_id" FROM "machine_token"
WHERE "workspace_id" = $1 AND "user_id" = $2
  AND "dev_machine_id" IS NOT NULL;

-- =====================================================================
-- M3. Machine list + annotations. runners.py:111-125
-- =====================================================================
SELECT dm.*,
       COUNT(DISTINCT r."id") FILTER (
           WHERE r."workspace_id" = $1 AND r."owner_id" = $2
             AND r."visibility" = 0) AS "runner_count",
       COUNT(DISTINCT r."id") FILTER (
           WHERE r."workspace_id" = $1 AND r."owner_id" = $2
             AND r."visibility" = 0 AND r."revoked_at" IS NULL
             AND r."status" IN ('online', 'busy')) AS "online_runner_count",
       MAX(r."last_heartbeat_at") FILTER (
           WHERE r."workspace_id" = $1 AND r."owner_id" = $2
             AND r."visibility" = 0) AS "last_heartbeat_at",
       <M1 exists> AS "control_online"
FROM "dev_machine" dm
LEFT OUTER JOIN "runner" r ON (dm."id" = r."dev_machine_id")
WHERE (dm."id" IN (<M2a>) OR dm."id" IN (<M2b>))
  AND dm."owner_id" = $2 AND dm."visibility" = 0
GROUP BY dm."id", dm."owner_id", dm."host_label", dm."label",
         dm."visibility", dm."provisioning", dm."last_seen_at",
         dm."revoked_at", dm."created_at", dm."updated_at"
ORDER BY dm."last_seen_at" DESC, dm."created_at" DESC;
-- Django emits the id lists as subqueries (QuerySet used as IN rhs), not
-- two round-trips: `IN (SELECT ... FROM runner ...)` / `IN (SELECT ... FROM
-- machine_token ...)`. NULL last_seen_at sorts FIRST under DESC (Postgres
-- default: NULLS FIRST on DESC) — port the ordering, not just the columns.
-- Serialized with DevMachineSerializer(many=True).

-- =====================================================================
-- M4. _machine_is_in_workspace_scope probes. runners.py:132-147
--     (can_view_dev_machine is pure-Python: owner match + PRIVATE.)
-- =====================================================================
SELECT EXISTS(
    SELECT 1 FROM "runner"
    WHERE "workspace_id" = $1 AND "owner_id" = $2
      AND "visibility" = 0 AND "dev_machine_id" = $3);
SELECT EXISTS(
    SELECT 1 FROM "machine_token"
    WHERE "workspace_id" = $1 AND "user_id" = $2 AND "dev_machine_id" = $3);
-- Either true (AND can_view) -> in scope. Checked AFTER the
-- select_for_update machine lock in revoke/rotate, BEFORE the service call
-- (unlocked read) in delete.

-- =====================================================================
-- M5. Single-machine serialize (revoke/rotate response). runners.py:150-170
-- =====================================================================
-- Same annotations as M3 but WHERE dm."id" = $pk, .first(). When the row
-- vanished (None), the serializer falls back to the UNANNOTATED instance ->
-- runner_count/online_runner_count/last_heartbeat_at keys OMITTED,
-- control_online false (see F2).

-- =====================================================================
-- M6. Machine revoke writes (inside tx). runners.py:189-208
-- =====================================================================
-- BEGIN;
SELECT * FROM "dev_machine" WHERE "id" = $1 LIMIT 1 FOR UPDATE;
-- out of scope -> 404 {"error": "not found"} (tx rolls back).
UPDATE "dev_machine" SET "revoked_at" = $2, "updated_at" = $2 WHERE "id" = $1;
-- (only when revoked_at was NULL; save(update_fields=[revoked_at, updated_at]))
UPDATE "machine_token" SET "revoked_at" = $2
WHERE "dev_machine_id" = $1 AND "revoked_at" IS NULL;
SELECT * FROM "runner" WHERE "dev_machine_id" = $1 AND "revoked_at" IS NULL
FOR UPDATE;
-- then per runner: send_runner_revoke (Redis) BEFORE runner.revoke cascade
-- (see revoke_cascade.sql) + close_runner_session; COMMIT.
-- Response: 200 + M5 serialization.

-- =====================================================================
-- M7. Machine rotate writes (inside tx). runners.py:227-246
-- =====================================================================
-- BEGIN; locked machine read as M6; out of scope -> 404 "not found".
-- machine.revoked_at set -> 409 {"error": "dev_machine_revoked"}.
UPDATE "machine_token" SET "revoked_at" = $2
WHERE "dev_machine_id" = $1 AND "revoked_at" IS NULL;
SELECT "id" FROM "runner"
WHERE "dev_machine_id" = $1 AND "revoked_at" IS NULL;
-- per id: send_runner_revoke (Redis) + close_runner_session (row revoke).
-- NOTE: runners are NOT Runner.revoke()d here — their status/revoked_at
-- columns are untouched; only their SESSIONS die. COMMIT.
-- Response: 200 + M5 serialization.

-- =====================================================================
-- R1. Runner list. runners.py:311-333
-- =====================================================================
SELECT r.*, p.*, pr.*, dm.*
FROM "runner" r
INNER JOIN "pod" p ON (r."pod_id" = p."id")
INNER JOIN "projects" pr ON (p."project_id" = pr."id")
LEFT OUTER JOIN "dev_machine" dm ON (r."dev_machine_id" = dm."id")
WHERE r."workspace_id" = $1
  AND r."owner_id" = $2 AND r."visibility" = 0   -- runner_visible_to_user_q
  -- [+ AND r."pod_id" = $3]                       when ?pod=
  -- [+ AND r."provisioning" <> 'desktop_bundled'] unless ?include_bundled=1/true/yes
  -- [+ AND p."project_id" = $4]                   when ?project=
ORDER BY r."updated_at" DESC;
-- live_state is NOT prefetched (N+1 per row on serialize — port the SHAPE,
-- not the query count). Missing ?workspace= -> 400; non-member -> 403.

-- =====================================================================
-- R2. Runner detail/patch read. runners.py:342-352
-- =====================================================================
SELECT r.*, p.*, pr.*, dm.*
FROM "runner" r
INNER JOIN "pod" p ON (r."pod_id" = p."id")
INNER JOIN "projects" pr ON (p."project_id" = pr."id")
LEFT OUTER JOIN "dev_machine" dm ON (r."dev_machine_id" = dm."id")
WHERE r."id" = $1 LIMIT 1;
-- None -> 404 {"error": "not found"}. Non-member of runner.workspace ->
-- 403 {"error": "forbidden"}. can_view_runner false (non-owner) -> 404
-- (existence not leaked to non-owners). PATCH additionally requires
-- can_manage_runner (owner) else 403.

-- =====================================================================
-- R3. Runner PATCH pod-move (inside tx) + busy guard. runners.py:384-422
-- =====================================================================
-- BEGIN (only when "pod" in body);
SELECT * FROM "pod" WHERE "id" = $1 LIMIT 1 FOR UPDATE;
-- pod None (missing OR soft-deleted — default manager) ->
--   400 {"error": "pod does not exist or has been deleted"}.
-- pod.workspace_id <> runner.workspace_id ->
--   400 {"error": "pod is in a different workspace"}.
-- Busy guard — ONLY when new_pod.id != runner.pod_id (:409-412):
SELECT EXISTS(
    SELECT 1 FROM "agent_run"
    WHERE ("runner_id" = $2 OR "pinned_runner_id" = $2)
      AND "status" IN ('queued','assigned','waiting_for_worktree','running',
                       'cancel_requested','awaiting_approval','awaiting_reauth',
                       'paused_awaiting_input'));
-- True -> 409 {"error": "runner has an in-flight or queued run; wait for it to finish or cancel it first",
--              "code": "runner_busy"}.
-- Write (:422; name may ride along when both keys present):
UPDATE "runner" SET "pod_id" = $1 [, "name" = $3], "updated_at" = now()
WHERE "id" = $2;
-- COMMIT. Name-only PATCH (no "pod" key): same UPDATE without pod_id and
-- without tx (:423-424). Empty name -> 400 {"error": "name cannot be empty"}.
-- Response: 200 + full RunnerSerializer.

-- =====================================================================
-- R4. Runner revoke (row-locked read-then-revoke). runners.py:475-491
-- =====================================================================
-- BEGIN;
SELECT * FROM "runner" WHERE "id" = $1 LIMIT 1 FOR UPDATE;
-- None / non-viewable -> 404 "not found"; non-manageable -> 403.
-- already_revoked = (revoked_at IS NOT NULL).
-- When not already revoked: Runner.revoke('manual_revoke') cascade
-- (see revoke_cascade.sql) — COMMIT — then send_runner_revoke('revoked by
-- user') + close_runner_session + refresh_from_db (re-read row).
-- When already revoked: NO frame, NO close; COMMIT; serialize as-is.
-- Response: 200 + full RunnerSerializer either way.
