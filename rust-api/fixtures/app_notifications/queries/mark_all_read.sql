-- FX-NOTIF-08 — MarkAllReadNotificationViewSet.create bulk write.
-- Trace: apps/api/pi_dash/app/views/notification/base.py:232-288 (create :233-288;
-- route POST workspaces/<slug>/users/notifications/mark-all-read/
-- app/urls/notification.py:42-46). Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.
-- Base: receiver + read_at IS NULL only (:239-243) — NO entity_name guard, NO
-- mentioned handling, NO read filter. Response always {"message": "Successful"} 200 (:288).

-- Snoozed filter (:246-249). NOTE the inversion vs list: here `snoozed` comes from
-- request DATA (default False, real booleans — :235), and the TRUE branch repeats
-- the same over-broad OR as list (:247):
--   snoozed truthy:  AND ("snoozed_till" < now OR "snoozed_till" IS NOT NULL)
--   snoozed falsy:   AND ("snoozed_till" >= now OR "snoozed_till" IS NULL)

-- Archived filter (:252-255; data default False):
--   archived truthy: AND "archived_at" IS NOT NULL
--   archived falsy:  AND "archived_at" IS NULL

-- Type filter (:258-281): EXACT match on "watching"/"assigned"/"created"
-- (NOT the list's comma-split subscribed/assigned/created — port the spelling
-- difference as-is). "all" (default) = no clause:
--   watching (:258-262): entity_identifier IN (
--     SELECT issue_id FROM issue_subscribers WHERE workspace + subscriber = user)
--     — PLAIN subscriber list, no created/assigned exclusion unlike list :108-114.
--   assigned (:265-269): entity_identifier IN (
--     SELECT issue_id FROM issue_assignees WHERE workspace + assignee = user)
--   created (:272-281): WorkspaceMember role<15 exists -> .none() (observed, as list);
--     else entity_identifier IN (SELECT id FROM issues WHERE workspace + created_by = user)

-- Write (:283-287): Python loop sets read_at = timezone.now() per row (:285 —
-- distinct microseconds-apart timestamps, NOT one shared value), then
-- Notification.objects.bulk_update(rows, ["read_at"],
-- batch_size=100) — batched UPDATEs of 100, signals/save() NOT run, updated_at
-- auto_now NOT refreshed by bulk_update. Empty set -> bulk_update([]) no-op, still 200.
-- Equivalent per-batch SQL (values Навального-style):
-- UPDATE "notifications" SET "read_at" = %(now)s WHERE "id" IN (...100 ids...);
