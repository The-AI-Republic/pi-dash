-- FX-NOTIF-06 — NotificationViewSet.list queryset.
-- Trace: apps/api/pi_dash/app/views/notification/base.py:57-137 (intake Exists :57-61,
-- base queryset + annotations :63-77, snoozed :80-85, archived :87-92, read :94-98,
-- mentioned :100-103, type subqueries :105-135, apply :136-137; pagination :140-146,
-- plain serialize :148-149). Route: GET workspaces/<slug>/users/notifications/
-- (app/urls/notification.py:17-21). Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.

-- Base queryset (:63-77). NOTE the double Exists annotation (:66-67): is_inbox_issue
-- and is_intake_issue annotate the SAME intake subquery; port both columns as-is.
-- Ordering is ("snoozed_till", "-created_at") — snoozed_till ASC first, NULLS
-- default (Postgres: NULLS LAST on ASC), then created_at DESC.
SELECT "notifications"."id",
       EXISTS (SELECT 1 FROM "issues" U0
               INNER JOIN "issue_intake" U1 ON (U0."id" = U1."issue_id")
               INNER JOIN "workspaces" U2 ON (U0."workspace_id" = U2."id")
               WHERE (U0."id" = "notifications"."entity_identifier"
                      AND U1."status" IN (0, 2, -2)
                      AND U2."slug" = %(slug)s)) AS "is_inbox_issue",
       EXISTS (SELECT 1 FROM "issues" U0
               INNER JOIN "issue_intake" U1 ON (U0."id" = U1."issue_id")
               INNER JOIN "workspaces" U2 ON (U0."workspace_id" = U2."id")
               WHERE (U0."id" = "notifications"."entity_identifier"
                      AND U1."status" IN (0, 2, -2)
                      AND U2."slug" = %(slug)s)) AS "is_intake_issue",
       CASE WHEN "notifications"."sender" ILIKE '%mentioned%' THEN true ELSE false END
         AS "is_mentioned_notification"
  FROM "notifications"
  INNER JOIN "workspaces" ON ("notifications"."workspace_id" = "workspaces"."id")
 WHERE ("workspaces"."slug" = %(slug)s
        AND "notifications"."receiver_id" = %(user_id)s
        AND "notifications"."entity_name" = 'issue'          -- :65 entity guard
        AND "notifications"."deleted_at" IS NULL)            -- SoftDeletionManager
 ORDER BY "notifications"."snoozed_till" ASC, "notifications"."created_at" DESC;

-- Snoozed filter (:80-85; default param "false"). OBSERVED (suspected bug, do NOT fix):
-- the "true" branch (snoozed_till < now OR snoozed_till IS NOT NULL) matches every
-- row whose snoozed_till is set — including future-snoozed rows — so snoozed=true
-- returns almost everything. Port verbatim.
--   "true":  AND ("notifications"."snoozed_till" < %(now)s OR "notifications"."snoozed_till" IS NOT NULL)
--   "false": AND ("notifications"."snoozed_till" >= %(now)s OR "notifications"."snoozed_till" IS NULL)

-- Archived filter (:87-92; default "false"):
--   "true":  AND "notifications"."archived_at" IS NOT NULL
--   "false": AND "notifications"."archived_at" IS NULL

-- Read filter (:94-98; default None = no clause):
--   read=false: AND "notifications"."read_at" IS NULL
--   read=true:  AND "notifications"."read_at" IS NOT NULL

-- Mentioned filter (:100-103). NOTE: request.GET.get("mentioned", False) returns the
-- STRING "false"/"true" when passed, both truthy in Python — so mentioned=false in the
-- query string still takes the `if mentioned:` branch (icontains filter). Only an
-- ABSENT param takes the exclude branch (the contract-suite default). Port as-is:
--   param absent -> AND NOT ("notifications"."sender" ILIKE '%mentioned%')
--   param present (any value incl. "false") -> AND ("notifications"."sender" ILIKE '%mentioned%')

-- Type filter (:105-137): `type` split on ","; default "all" matches no branch, so
-- q_filters stays empty and .filter(Q()) is a no-op. Branches OR together:
--   subscribed (:107-115): entity_identifier IN (
--     SELECT U0."issue_id" FROM "issue_subscribers" U0
--     WHERE (U0."workspace_id" = %(ws)s AND U0."subscriber_id" = %(user_id)s
--       AND NOT EXISTS (SELECT 1 FROM "issues" WHERE created_by_id = %(user_id)s AND id = U0."issue_id")
--       AND NOT EXISTS (SELECT 1 FROM "issue_assignees" WHERE id = U0."issue_id" AND assignee_id = %(user_id)s)))
--   assigned (:118-122): entity_identifier IN (
--     SELECT "issue_id" FROM "issue_assignees" WHERE workspace_id = %(ws)s AND assignee_id = %(user_id)s)
--   created (:125-134): if EXISTS (SELECT 1 FROM "workspace_members"
--     WHERE workspace_id = %(ws)s AND member_id = %(user_id)s AND role < 15 AND is_active)
--     THEN queryset = .none() — members with a sub-15 role see NOTHING on type=created
--     (observed, port as-is); ELSE entity_identifier IN (
--     SELECT "id" FROM "issues" WHERE workspace_id = %(ws)s AND created_by_id = %(user_id)s)

-- Pagination (:140-146): only when BOTH per_page and cursor params present does the
-- BasePaginator envelope apply (order_by defaults to "-created_at"); otherwise the
-- plain list serializes the FULL filtered queryset with NotificationSerializer (:148).
