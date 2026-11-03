-- queries/instance_first.sql
-- Call sites: Instance.objects.first() in api/views/instance.py:37 (GET),
-- :178 (PATCH), :191 (signup-visited); api/views/admin.py:56,72,84;
-- api/permissions/instance.py:17; bgtasks/tracer.py:31;
-- management/commands/register_instance.py:55.
-- Emitted by the Django query compiler (settings.test, no DB). first()
-- slices [:1], hence LIMIT 1. Default manager adds deleted_at IS NULL
-- (db/mixins.py:52-54); ORDER BY from Meta.ordering = ("-created_at",).
SELECT "instances"."created_at", "instances"."updated_at", "instances"."created_by_id", "instances"."updated_by_id", "instances"."deleted_at", "instances"."id", "instances"."instance_name", "instances"."whitelist_emails", "instances"."instance_id", "instances"."current_version", "instances"."latest_version", "instances"."edition", "instances"."domain", "instances"."last_checked_at", "instances"."namespace", "instances"."is_telemetry_enabled", "instances"."is_support_required", "instances"."is_setup_done", "instances"."is_signup_screen_visited", "instances"."is_verified", "instances"."is_test", "instances"."is_current_version_deprecated" FROM "instances" WHERE "instances"."deleted_at" IS NULL ORDER BY "instances"."created_at" DESC LIMIT 1;
