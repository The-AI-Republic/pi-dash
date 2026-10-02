-- queries/scheduler_sql.sql
-- Scheduler definition reads/writes record: SQL shape + write effects.
-- Source: app/views/scheduler/views.py:46-157. Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.
-- Method: str(queryset.query) under pi_dash.settings.test (Postgres dialect,
-- no DB round-trip); literals replaced with :placeholders below.
--
-- R1 list (:59-68): workspace scope via resolved Workspace row, annotation,
--   ORDER BY name. Soft-deleted schedulers excluded by the default
--   SoftDeletionManager (deleted_at IS NULL).
-- R2 detail (:98-107, shared by GET/PATCH/DELETE-lookup): same annotation,
--   pk + workspace__slug scope (INNER JOIN workspaces). DELETE's own lookup
--   (:139-141) skips the annotation (no serialization afterwards).
-- R3 create (:79-84): INSERT with provided fields + model defaults; the
--   response re-render is NOT annotated -> active_binding_count falls back
--   to the R6 COUNT (always 0 for a new row).
-- R4 PATCH (:127-133): full-column UPDATE (Django Model.save writes every
--   column) + auto_now updated_at + crum updated_by; response re-renders the
--   in-memory row which keeps the PRE-SAVE annotation (accurate: PATCH never
--   touches bindings).
-- R5 DELETE cascade (:150-157): one transaction { UPDATE active bindings SET
--   deleted_at, soft-delete scheduler }; the scheduler .delete() ALSO fires
--   the async soft_delete_related_objects celery task (db/mixins.py:72-78) -
--   the inline UPDATE exists so the API view is consistent immediately.
-- R6 fallback COUNT (serializers/scheduler.py:108): COUNT(*) over the
--   scheduler's non-deleted bindings (manager NULL-guard + explicit NULL-guard).

-- R1 scheduler list (:59-68)
SELECT "schedulers"."created_at", "schedulers"."updated_at", "schedulers"."created_by_id", "schedulers"."updated_by_id", "schedulers"."deleted_at", "schedulers"."id", "schedulers"."workspace_id", "schedulers"."slug", "schedulers"."name", "schedulers"."description", "schedulers"."prompt", "schedulers"."source", "schedulers"."is_enabled", "schedulers"."color", COUNT("scheduler_bindings"."id") FILTER (WHERE "scheduler_bindings"."deleted_at" IS NULL) AS "_active_binding_count" FROM "schedulers" LEFT OUTER JOIN "scheduler_bindings" ON ("schedulers"."id" = "scheduler_bindings"."scheduler_id") WHERE ("schedulers"."deleted_at" IS NULL AND "schedulers"."workspace_id" = :workspace_id) GROUP BY "schedulers"."id" ORDER BY "schedulers"."name" ASC;

-- R2 scheduler detail (:98-107)
SELECT "schedulers"."created_at", "schedulers"."updated_at", "schedulers"."created_by_id", "schedulers"."updated_by_id", "schedulers"."deleted_at", "schedulers"."id", "schedulers"."workspace_id", "schedulers"."slug", "schedulers"."name", "schedulers"."description", "schedulers"."prompt", "schedulers"."source", "schedulers"."is_enabled", "schedulers"."color", COUNT("scheduler_bindings"."id") FILTER (WHERE "scheduler_bindings"."deleted_at" IS NULL) AS "_active_binding_count" FROM "schedulers" LEFT OUTER JOIN "scheduler_bindings" ON ("schedulers"."id" = "scheduler_bindings"."scheduler_id") INNER JOIN "workspaces" ON ("schedulers"."workspace_id" = "workspaces"."id") WHERE ("schedulers"."deleted_at" IS NULL AND "schedulers"."id" = :scheduler_id AND "workspaces"."slug" = :slug) GROUP BY "schedulers"."id";

-- R3 scheduler create (:79-84): INSERT columns = provided {slug, name, prompt, [description, color, is_enabled]} + workspace_id + id/created_at/updated_at (auto) + created_by_id (crum request user) + model defaults for omitted (description '', color '#3b82f6', source 'builtin', is_enabled true)
INSERT INTO "schedulers" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "workspace_id", "slug", "name", "description", "prompt", "source", "is_enabled", "color") VALUES (:id, :now, :now, :user_id, NULL, NULL, :workspace_id, :slug, :name, :description, :prompt, 'builtin', :is_enabled, :color);

-- R4 scheduler PATCH (:127-133): full-column UPDATE (provided values; untouched columns rewritten with their current values) + updated_at/updated_by refresh
UPDATE "schedulers" SET "updated_at" = :now, "updated_by_id" = :user_id, "slug" = :slug, "name" = :name, "description" = :description, "prompt" = :prompt, "color" = :color, "is_enabled" = :is_enabled WHERE "schedulers"."id" = :scheduler_id;

-- R5 scheduler DELETE cascade (:150-157): single transaction, bindings first
BEGIN;
UPDATE "scheduler_bindings" SET "deleted_at" = :now WHERE ("scheduler_bindings"."scheduler_id" = :scheduler_id AND "scheduler_bindings"."deleted_at" IS NULL);
-- NOTE: QuerySet.update() bypasses save()/auto_now: bindings' updated_at is NOT touched.
UPDATE "schedulers" SET "deleted_at" = :now, "updated_at" = :now2, "updated_by_id" = :user_id WHERE "schedulers"."id" = :scheduler_id;
-- plus: soft_delete_related_objects.delay('db', 'scheduler', :scheduler_id) enqueued (async cascade; the inline UPDATE above is what the API observes)
COMMIT;

-- R6 active_binding_count fallback (serializers/scheduler.py:108)
SELECT COUNT(*) AS "__count" FROM "scheduler_bindings" WHERE ("scheduler_bindings"."deleted_at" IS NULL AND "scheduler_bindings"."deleted_at" IS NULL AND "scheduler_bindings"."scheduler_id" = :scheduler_id);
