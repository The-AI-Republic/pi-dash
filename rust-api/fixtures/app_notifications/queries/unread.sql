-- FX-NOTIF-07 — UnreadNotificationEndpoint.get counts.
-- Trace: apps/api/pi_dash/app/views/notification/base.py:196-229 (class :196-197,
-- get :199-229; route GET workspaces/<slug>/users/notifications/unread/
-- app/urls/notification.py:37-41). Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.
-- Both counts share the base: receiver + unread + unarchived + unsnoozed. The split
-- is ONLY sender ILIKE '%mentioned%': exclude (:210) vs include (:220). There is NO
-- entity_name='issue' guard here (unlike list :65) — port as-is.

-- Q1 watching count (:202-212):
SELECT COUNT(*) AS "total_unread_notifications_count" FROM "notifications"
  INNER JOIN "workspaces" ON ("notifications"."workspace_id" = "workspaces"."id")
 WHERE ("workspaces"."slug" = %(slug)s
        AND "notifications"."receiver_id" = %(user_id)s
        AND "notifications"."read_at" IS NULL
        AND "notifications"."archived_at" IS NULL
        AND "notifications"."snoozed_till" IS NULL
        AND NOT ("notifications"."sender" ILIKE '%mentioned%')
        AND "notifications"."deleted_at" IS NULL);

-- Q2 mention count (:214-221):
SELECT COUNT(*) AS "mention_unread_notifications_count" FROM "notifications"
  INNER JOIN "workspaces" ON ("notifications"."workspace_id" = "workspaces"."id")
 WHERE ("workspaces"."slug" = %(slug)s
        AND "notifications"."receiver_id" = %(user_id)s
        AND "notifications"."read_at" IS NULL
        AND "notifications"."archived_at" IS NULL
        AND "notifications"."snoozed_till" IS NULL
        AND "notifications"."sender" ILIKE '%mentioned%'
        AND "notifications"."deleted_at" IS NULL);

-- Response (:223-229) is exactly:
--   {"total_unread_notifications_count": int(q1), "mention_unread_notifications_count": int(q2)}
-- status 200. Snoozed-till-in-future rows are excluded (IS NULL test, not a
-- comparison); read or archived rows are excluded even if otherwise matching.
