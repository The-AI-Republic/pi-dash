-- D13-F5 (part 2): MachineToken bootstrap/rotate + DevMachine get-or-create.
-- Base: apps/api/pi_dash/. Method: hand-composed from cited ORM calls.

-- =====================================================================
-- B1. Bootstrap probe: locked active token. enrollment.py:143-153
--     (called INSIDE the enrollment/creation tx).
-- =====================================================================
-- With a dev machine:
SELECT * FROM "machine_token"
WHERE "user_id" = $1 AND "workspace_id" = $2
  AND "revoked_at" IS NULL AND "dev_machine_id" = $3
LIMIT 1
FOR UPDATE;
-- Without (legacy host-label token):
SELECT * FROM "machine_token"
WHERE "user_id" = $1 AND "workspace_id" = $2
  AND "revoked_at" IS NULL AND "host_label" = $4
  AND "dev_machine_id" IS NULL
LIMIT 1
FOR UPDATE;
-- Hit -> return None (no mint; response omits machine_token).

-- =====================================================================
-- B2. Bootstrap insert (savepoint; IntegrityError -> None).
--     enrollment.py:157-171
-- =====================================================================
-- SAVEPOINT;
INSERT INTO "machine_token"
    ("id", "user_id", "dev_machine_id", "workspace_id", "host_label",
     "token_hash", "token_fingerprint", "label", "is_service",
     "created_at", "last_used_at", "revoked_at")
VALUES ($1, $2, $3, $4, $5, $6, $7, 'machine: ' || substr($4, 1, 96), TRUE,
        now(), NULL, NULL);
-- label is f"machine: {host_label[:96]}".
-- Unique violation (partial unique indexes) -> ROLLBACK TO SAVEPOINT, None.

-- =====================================================================
-- B3. Rotate: revoke-all + insert (NOT savepoint-guarded — caller tx).
--     enrollment.py:174-203. NOTE the filter asymmetry vs B1: with a dev
--     machine the user_id is NOT in the filter (workspace+machine only);
--     without, it is (user+workspace+host).
-- =====================================================================
-- With a dev machine:
UPDATE "machine_token" SET "revoked_at" = now()
WHERE "workspace_id" = $1 AND "revoked_at" IS NULL AND "dev_machine_id" = $2;
-- select_for_update() precedes the update (locked filter, then bulk update).
-- Without:
UPDATE "machine_token" SET "revoked_at" = now()
WHERE "workspace_id" = $1 AND "revoked_at" IS NULL
  AND "user_id" = $2 AND "host_label" = $3 AND "dev_machine_id" IS NULL;
INSERT INTO "machine_token" (...) VALUES (...);  -- same shape as B2.

-- =====================================================================
-- D1. Dev-machine by id (locked). enrollment.py:80-85
-- =====================================================================
SELECT * FROM "dev_machine" WHERE "id" = $1 LIMIT 1 FOR UPDATE;
-- Hit + owner mismatch -> DevMachineOwnershipError -> 404 dev_machine_not_found.
-- Hit + owner match -> _touch_dev_machine (see D4), return.

-- =====================================================================
-- D2. Dev-machine create with client id (savepoint). enrollment.py:86-99
-- =====================================================================
-- SAVEPOINT;
INSERT INTO "dev_machine"
    ("id", "owner_id", "host_label", "label", "visibility",
     "provisioning", "last_seen_at", "revoked_at",
     "created_at", "updated_at")
VALUES ($1, $2, $3, substr($3, 1, 128), 0, 'manual', now(), NULL, now(), now());
-- IntegrityError (id race) -> re-lock D1; miss/foreign owner -> ownership
-- error; else touch + return.

-- =====================================================================
-- D3. Dev-machine by (owner, host_label) (locked, oldest first).
--     enrollment.py:101-127. Empty host_label + no id -> return None
--     (runner.dev_machine stays NULL — legacy path).
-- =====================================================================
SELECT * FROM "dev_machine"
WHERE "owner_id" = $1 AND "host_label" = $2 AND "revoked_at" IS NULL
ORDER BY "created_at" ASC LIMIT 1
FOR UPDATE;
-- Hit -> touch + return. Miss -> INSERT as D2 with server uuid; on
-- IntegrityError (legacy owner/host constraint race) re-select WITHOUT
-- create and return whatever won (may be None).
SELECT * FROM "dev_machine"
WHERE "owner_id" = $1 AND "host_label" = $2 AND "revoked_at" IS NULL
ORDER BY "created_at" ASC LIMIT 1
FOR UPDATE;

-- =====================================================================
-- D4. _touch_dev_machine write. enrollment.py:57-69
-- =====================================================================
-- Always: last_seen_at=now, updated_at=now (update_fields includes both).
-- + host_label when non-empty request label differs from stored.
-- + label=host_label[:128] when non-empty request label AND stored label empty.
UPDATE "dev_machine"
SET "last_seen_at" = now(), "updated_at" = now()
    -- [, "host_label" = $1] [, "label" = $2]
WHERE "id" = $3;
-- host_label stored value: (request or '').strip()[:255].
