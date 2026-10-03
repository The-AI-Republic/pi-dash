-- D13-F5 (part 1): daemon enrollment reads/writes.
-- Base: apps/api/pi_dash/. $N are bind params. Method: hand-composed from the
-- cited ORM calls (no live Postgres on this runner — D-06/D-11 precedent);
-- every statement re-read against the source after composing.
-- Verified Django semantic (probe /tmp/d13_save.py, Django 6.0.5):
--   save(update_fields=[...]) does NOT bump auto_now updated_at unless listed;
--   QuerySet.update() never bumps it.

-- =====================================================================
-- E1. Enroll tx: locked runner by one-time enrollment hash (+ workspace,
--     pod, pod.project joins for the response body). enrollment.py:284-290
-- =====================================================================
-- BEGIN (transaction.atomic);
SELECT r."id", r."owner_id", r."workspace_id", r."dev_machine_id", r."pod_id",
       r."name", r."host_label", r."provisioning", r."visibility",
       r."refresh_token_hash", r."refresh_token_fingerprint",
       r."refresh_token_generation", r."previous_refresh_token_hash",
       r."access_token_signing_key_version",
       r."enrollment_token_hash", r."enrollment_token_fingerprint",
       r."enrolled_at", r."capabilities", r."status", r."os", r."arch",
       r."runner_version", r."dev_metadata", r."protocol_version",
       r."last_heartbeat_at", r."free_worktrees",
       r."created_at", r."updated_at", r."revoked_at", r."revoked_reason",
       w."id", w."slug", p."id", p."name", pr."id", pr."identifier"
FROM "runner" r
INNER JOIN "workspaces" w ON (r."workspace_id" = w."id")
INNER JOIN "pod" p ON (r."pod_id" = p."id")
INNER JOIN "projects" pr ON (p."project_id" = pr."id")
WHERE r."enrollment_token_hash" = $1
FOR UPDATE;
-- Miss -> 401 {"error": "invalid_or_expired_enrollment_token"} (tx rolls back).
-- revoked_at set -> 409 {"error": "runner_revoked"}.
-- enrolled_at set -> 409 {"error": "enrollment_token_already_used"}.

-- =====================================================================
-- E2. Enroll tx: mark enrolled + rotate to refresh generation 1.
--     enrollment.py:325-348. NOTE: updated_at NOT in update_fields ->
--     updated_at UNCHANGED by this write.
-- =====================================================================
UPDATE "runner"
SET "dev_machine_id" = $1,          -- may be NULL (no id + no host_label)
    "host_label" = $2,              -- request host_label, or old value when blank
    "enrolled_at" = $3,             -- now()
    "enrollment_token_hash" = '',
    "enrollment_token_fingerprint" = '',
    "refresh_token_hash" = $4,      -- freshly minted rt_ hash
    "refresh_token_fingerprint" = $5,
    "refresh_token_generation" = 1,
    "previous_refresh_token_hash" = ''
    -- + "name" = $6 ONLY when the body supplied a non-blank name
WHERE "id" = $7;

-- =====================================================================
-- E3. Refresh row-lock: locked runner + workspace. enrollment.py:403-404
-- =====================================================================
-- BEGIN (transaction.atomic);
SELECT r.*, w.*
FROM "runner" r
INNER JOIN "workspaces" w ON (r."workspace_id" = w."id")
WHERE r."id" = $1
FOR UPDATE;
-- Miss -> 401 {"error": "invalid_refresh_token"} (NULL runner, same code as
-- bad-hash: existence is not leaked). revoked -> 401 {"error": "runner_revoked"}.

-- =====================================================================
-- E4. Refresh: dev-machine revoked probe (NOT via the join — separate
--     EXISTS). enrollment.py:415-425
-- =====================================================================
SELECT EXISTS(
    SELECT 1 FROM "dev_machine"
    WHERE "id" = $1 AND "revoked_at" IS NOT NULL
);
-- True -> 401 {"error": "dev_machine_revoked"}.

-- =====================================================================
-- E5. Refresh: rotate (happy path). enrollment.py:448-460 + 467.
--     NOTE: updated_at UNCHANGED (update_fields list has no updated_at).
-- =====================================================================
UPDATE "runner"
SET "previous_refresh_token_hash" = "refresh_token_hash",
    "refresh_token_hash" = $1,      -- new rt_ hash
    "refresh_token_fingerprint" = $2,
    "refresh_token_generation" = "refresh_token_generation" + 1
WHERE "id" = $3;
-- then, still inside the tx:
DELETE FROM "runner_force_refresh" WHERE "runner_id" = $3;
-- COMMIT. Previous-hash replay INSTEAD calls runner.revoke('refresh_token_replayed')
-- (see revoke_cascade.sql) and returns 401 {"error": "refresh_token_replayed"}.
-- Non-member INSTEAD calls runner.revoke('membership_revoked') + 401.

-- =====================================================================
-- E6. Create-endpoint reads. enrollment.py:629-680
-- =====================================================================
-- E6a. Explicit workspace_slug (:630):
SELECT * FROM "workspaces" WHERE "slug" = $1 LIMIT 1;
-- Miss (or caller not a member — same 404, no leak):
--   404 {"error": "workspace_not_found"}.

-- E6b. Inferred workspace (:639-643): caller's active memberships + workspace
--      join, oldest first, probe 2 rows:
SELECT wm.*, w.*
FROM "workspace_members" wm
INNER JOIN "workspaces" w ON (wm."workspace_id" = w."id")
WHERE wm."member_id" = $1 AND wm."is_active" = TRUE
ORDER BY wm."created_at" ASC
LIMIT 2;
-- 0 rows -> 400 {"error": "no_workspace_membership", ...};
-- 2 rows -> 400 {"error": "workspace_slug_required", ...}.

-- E6c. Project (:664):
SELECT * FROM "projects"
WHERE "workspace_id" = $1 AND "identifier" = $2 LIMIT 1;
-- Miss -> 404 {"error": "project_not_found"}.

-- E6d. Pod: explicit name first (:673), else project default (:675):
SELECT * FROM "pod"
WHERE "project_id" = $1 AND "name" = $2 AND "deleted_at" IS NULL LIMIT 1;
-- NOTE: when pod_name is given but matches nothing, `pod` stays None and the
-- code FALLS THROUGH to the default pod (no 404!) — an unknown ?pod= is
-- silently ignored (port as-is).
SELECT * FROM "pod"
WHERE "project_id" = $1 AND "is_default" = TRUE AND "deleted_at" IS NULL
ORDER BY "created_at" ASC LIMIT 1;
-- (default manager ordering -is_default,created_at; .first())
-- Still None -> 409 {"error": "project_has_no_default_pod"}.

-- =====================================================================
-- E7. Create tx body. enrollment.py:691-744 (inside retry loop, tx each try)
-- =====================================================================
-- _get_or_create_dev_machine (see dev_machine.sql D1-D3), then:
-- desktop_bundled reuse probe (:709-720):
SELECT * FROM "runner"
WHERE "owner_id" = $1 AND "workspace_id" = $2 AND "dev_machine_id" = $3
  AND "pod_id" = $4 AND "provisioning" = 'desktop_bundled'
  AND "revoked_at" IS NULL
LIMIT 1
FOR UPDATE;
-- Hit -> reuse (break loop; machine_minted stays None).
-- Miss -> _managed_cap_error count (see flows):
SELECT COUNT(*) FROM "runner"
WHERE "owner_id" = $1 AND "pod_id" = $2 AND "workspace_id" = $3
  AND "provisioning" = 'desktop_bundled' AND "status" <> 'revoked';
-- count >= settings.MANAGED_RUNNER_MAX_PER_USER_PROJECT (default 1)
--   -> 409 {"error": "managed_runner_limit", ...} (tx rolls back).
-- Insert (:726-735):
INSERT INTO "runner"
    ("id", "owner_id", "workspace_id", "dev_machine_id", "pod_id", "name",
     "host_label", "provisioning", "visibility",
     "refresh_token_hash", "refresh_token_fingerprint",
     "refresh_token_generation", "previous_refresh_token_hash",
     "access_token_signing_key_version",
     "enrollment_token_hash", "enrollment_token_fingerprint",
     "enrolled_at", "capabilities", "status", "os", "arch",
     "runner_version", "dev_metadata", "protocol_version",
     "last_heartbeat_at", "free_worktrees",
     "created_at", "updated_at", "revoked_at", "revoked_reason")
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0,
        '', '', 0, '', 1, '', '', now(), '[]', 'offline', '', '',
        '', '{}', 1, NULL, NULL, now(), now(), NULL, '');
-- IntegrityError (pod,name unique) -> explicit name: 409 runner_name_taken;
-- auto name: retry (max 5) else 409 could_not_allocate_runner_name.
-- APIToken callers (auth_machine_token None) + host_label: _rotate_machine_token
-- (see bootstrap.sql B3) + deactivate_api_token(request.auth) (see wire pins).
