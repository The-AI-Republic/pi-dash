# TRACE — D-34 app notifications fixtures (PIDASHCONV-297)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
Out of scope: `bgtasks/notification_task.py` + `bgtasks/email_notification_task.py` (D-07).
Routes (all 7): `app/urls/notification.py:16-52` — list `:17-21`, detail
`:22-26`, read `:27-31`, archive `:32-36`, unread `:37-41`, mark-all-read
`:42-46`, preferences `:47-51`. Permissions on every view are
`allow_permission(ADMIN, MEMBER, GUEST, WORKSPACE)` — recorded in PIDASHCONV-300,
not here.

## Models

- `models/notification.columns.json` — FX-NOTIF-01: `Notification`
  `db/models/notification.py:13-65` (columns `:14-33`, Meta + 9 indexes
  `:35-65`); `__str__` `:67-69`; audit cols `db/models/base.py:18`,
  `db/mixins.py:16-20,:26-42,:61-67`; default `message_html="<p></p>"` `:21`,
  ordering `-created_at` `:39`.
- `models/preference.columns.json` — FX-NOTIF-02: `UserNotificationPreference`
  `db/models/notification.py:81-114` + `get_default_preference()` golden
  `:72-78` (key/divergence note vs model columns recorded in-file).
- `models/email_log.columns.json` — FX-NOTIF-03: `EmailNotificationLog`
  `db/models/notification.py:121-149`, read-only reference (writer in D-07).

## Serializers

- `serializers/notification.golden.json` — FX-NOTIF-04: `NotificationSerializer`
  `app/serializers/notification.py:14-22` (nested `triggered_by_details`
  `UserLiteSerializer` `app/serializers/user.py:141-153`, read-only
  annotations); key set pinned by
  `rust-api/contract-tests/app_notifications/test_list.py::test_list_item_shape`.
- `serializers/preference.golden.json` — FX-NOTIF-05: serializer
  `app/serializers/notification.py:25-28`; shape pinned by contract
  `test_preferences.py::test_preferences_get_shape`.

## Queries

- `queries/list.sql` + `queries/list.rows.json` — FX-NOTIF-06: list queryset
  `app/views/notification/base.py:57-137` (intake Exists `:57-61`, base +
  annotations `:63-77`, snoozed `:80-85`, archived `:87-92`, read `:94-98`,
  mentioned `:100-103`, type subqueries `:105-135`, apply `:137`; serialize
  `:140-149`). Observed bugs ported as-is: double `Exists` (`:66-67`),
  over-broad `snoozed=true` branch (`:81`), `mentioned` string-truthiness
  (`:54,:100`), `type=created` member `.none()` (`:126-129`).
- `queries/unread.sql` + `queries/unread.rows.json` — FX-NOTIF-07:
  `UnreadNotificationEndpoint.get` `app/views/notification/base.py:196-229`
  (counts `:202-221`, body `:223-229`); no `entity_name` guard (as-is).
- `queries/mark_all_read.sql` + `queries/mark_all_read.rows.json` — FX-NOTIF-08:
  `MarkAllReadNotificationViewSet.create` `app/views/notification/base.py:232-288`
  (base `:239-243`, snoozed `:246-249`, archived `:252-255`, type `:258-281`,
  loop + `bulk_update(["read_at"], batch_size=100)` `:283-287`); `type`
  spelling `watching/assigned/created` vs list's `subscribed/assigned/created`
  recorded as-is.
- `queries/single_row.golden.json` — FX-NOTIF-09: `partial_update`
  `app/views/notification/base.py:152-161` (only `snoozed_till` honoured —
  observed bug vs comment `:154`), `mark_read/mark_unread` `:164-177`,
  `archive/unarchive` `:180-193`, preference get/patch `:296-308`
  (no get-or-create, `.get()` 500 on missing row — as-is).

## Production / review time per fixture (same-session blocks)

Hand-written against the sources above (no Django test client, no
record-and-freeze — each golden was composed from the cited lines, then
re-read against them).

| Fixture | Production | Review | Reviewer pass |
|---|---|---|---|
| models/notification.columns.json | ~10 min | ~5 min | cols vs model :14-33, index names :40-64 |
| models/preference.columns.json | ~8 min | ~4 min | cols :103-108, dict :72-78 verbatim |
| models/email_log.columns.json | ~5 min | ~3 min | cols :133-143 |
| serializers/notification.golden.json | ~12 min | ~6 min | keys vs contract test_list_item_shape |
| serializers/preference.golden.json | ~5 min | ~3 min | keys vs test_preferences_get_shape |
| queries/list.sql | ~25 min | ~12 min | branches vs base.py:57-137, OR structure |
| queries/list.rows.json | ~10 min | ~5 min | row visibility vs filter branches |
| queries/unread.sql + .rows.json | ~8 min | ~4 min | counts vs :202-221, response keys |
| queries/mark_all_read.sql + .rows.json | ~12 min | ~6 min | filters vs :239-281, bulk note :283-287 |
| queries/single_row.golden.json | ~12 min | ~6 min | each op vs :152-193,:296-308 |
| TRACE.md (this file) | ~10 min | ~4 min | every cited line re-grepped before commit |
| Total | ~117 min production | ~58 min review | |
