# TRACE — D-08 tasks_webhooks fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(zero drift verified: `git diff <ported-from> -- <8 files>` empty, line counts match).
Notes: the issue body says "28-type ACTIVITY_MAPPER" and "all 14 FX ids";
the sources enumerate 27 mapper keys and 13 FX ids — the 27 keys / 13 files
below are authoritative (per the epic split-review comment on PIDASHCONV-45).

How values were produced: rows marked `"executed": true` ran against
verbatim-extracted function bodies (AST pull, annotations stripped for py3.9;
Django/model seams stubbed, DNS + `requests.*` mocked) via throwaway probes
(`/tmp/fx_webhooks_probe.py`, `/tmp/fx_probe2.py` — not committed).
DB/network branches are transcribed with exact code refs; the PIDASHCONV-21
worker-plane oracle re-verifies them against live Django at the domain gate.

Reading material only: `tests/unit/bg_tasks/test_work_item_link_task.py` (126).

## FX-WEB-01 save_webhook_log

- `fx-web-01-save-webhook-log.json` — `bgtasks/webhook_task.py:93-140`
  (log_data shape `:107-118`, mongo branch `:120-130`, postgres fallback
  `:132-140`); `WebhookLog` columns `db/models/webhook.py:65-89`; base cols
  `db/models/base.py:18`, `db/mixins.py:19-20,29-36,61-64`.

## FX-WEB-02 get_model_data

- `fx-web-02-get-model-data.json` — `bgtasks/webhook_task.py:58-80`
  (SERIALIZER_MAPPER/MODEL_MAPPER), `:86-90` (get_issue_prefetches),
  `:143-187` (dispatch, issue prefetch + expand context, ValueError /
  ObjectDoesNotExist cases). Serializers live in other domains — class
  bindings only, no duplication.

## FX-WEB-03 webhook_send_task

- `fx-web-03-webhook-send-task.json` — `bgtasks/webhook_task.py:189-251`
  (send_webhook_deactivation_email), `:253-375` (task options `:253-259`,
  lookup `:283`, headers `:285-290`, DjangoJSONEncoder round-trip `:293-295`,
  action map `:297-302`, envelope `:304-311`, HMAC `:313-321`, post `:329`,
  success/failure logging `:332-356`, max-retries deactivation `:359-369`).
  HMAC hex + action map EXECUTED (stdlib expressions).

## FX-WEB-04 webhook_activity + model_activity

- `fx-web-04-webhook-fanout.json` — `bgtasks/webhook_task.py:377-461`
  (flag filter `:420-433`, per-webhook delay `:435-451`, deleted-verb
  `{id}` rule `:440`, exception rule `:453-460`); `:463-506` (created
  branch `:466-480`, per-key diff `:486-504`, absent-key skip quirk).

## FX-ACT-01 track_* helpers

- `fx-act-01-track-helpers.json` — `bgtasks/issue_activities_task.py:41-49`
  (extract_ids, EXECUTED incl. primary-presence-wins), `:50-77` (name),
  `:78-114` (description touch-or-append), `:115-160` (parent),
  `:161-188` (priority), `:189-229` (state), `:230-259` (target_date),
  `:260-289` (start_date, trailing-space comment), `:290-356` (labels),
  `:357-431` (assignees + IssueSubscriber bulk_create `:408`),
  `:433-477` (estimate_points, removed-verb + estimate.type crash),
  `:478-526` (archive_at 3-way), `:527-556` (closed_to). Pure rows EXECUTED.

## FX-ACT-02 activity builders

- `fx-act-02-activity-builders.json` — `bgtasks/issue_activities_task.py:557-665`
  (issue create/update/delete + ISSUE_ACTIVITY_MAPPER 17 keys `:604-622`),
  `:666-754` (comment), `:755-860` (cycle_issue, double-encoded created list),
  `:861-927` (module_issue), `:928-1016` (link, current-id reuse `:983`),
  `:1017-1069` (attachment, reads current_instance `:1039-1040`),
  `:1070-1139` (issue_reaction), `:1140-1216` (comment_reaction, unguarded
  unpack `:1152-1160`), `:1217-1276` (vote, created→verb updated),
  `:1277-1379` (relation pairs + inverse map), `:1380-1465` (draft 3-way),
  `:1466-1501` (intake create-only, int verb, status_dict).

## FX-ACT-03 issue_activity dispatcher

- `fx-act-03-dispatcher.json` — `bgtasks/issue_activities_task.py:1502-1604`
  (ACTIVITY_MAPPER 27 keys `:1540-1568`, uuid gate `:1521`, redis set ex=600
  `:1528-1531`, issue touch `:1532-1538`, bulk_create `:1584`,
  notifications.delay payload `:1586-1599`, broad-except `:1602-1604`);
  `notifications()` signature `bgtasks/notification_task.py:191-200`;
  `IssueActivitySerializer` shape `app/serializers/issue.py:521-539`.

## FX-AUTO-01 archive/close automation

- `fx-auto-01-archive-close.json` — `bgtasks/issue_automation_task.py:23-26`
  (entrypoint), `:29-88` (archive query `:39-55`, bulk `:69`, delay `:70-83`),
  `:90-150` (close query `:100-116`, cancelled fallback `:120-123`, bulk
  `:132`, delay `:133-146`); CLOSED/OPEN groups `utils/constants.py:76-88`;
  beat name+schedule `pi_dash/celery.py:42-45` (schedule owned by D-10).

## FX-LINK-01 link crawler

- `fx-link-01-crawler.json` — `bgtasks/work_item_link_task.py:24`
  (DEFAULT_FAVICON), `:27-71` (validate_url_ip, EXECUTED matrix),
  `:72-117` (safe_get, EXECUTED incl. 5-redirect pin), `:118-172` (crawl
  shapes), `:173-218` (find_favicon_url, EXECUTED incl. private-href
  propagation), `:219-260` (fetch_and_encode_favicon), `:262-273` (task,
  DoesNotExist path); `IssueLink.metadata` `db/models/issue.py:471-481`.

## FX-VISIT-01 recent_visited

- `fx-visit-01-recent-visited.json` — `bgtasks/recent_visited_task.py:17-61`
  (lookup `:20-27`, update+DatabaseError `:29-35`, ==20 eviction `:37-44`,
  create+backfill `:46-56`, outer except `:59-61`); `UserRecentVisit`
  columns `db/models/recent_visit.py:22-39`, base
  `db/models/workspace.py:185-195`.

## FX-PAGE-01 page_transaction

- `fx-page-01-page-transaction.json` — `bgtasks/page_transaction_task.py:21-43`
  (COMPONENT_MAP, entity_type-always-None), `:45-73` + `:74-82` (extract +
  get_entity_details, EXECUTED vectors), `:84-142` (backfill skip `:112`,
  bulk_create batch 50 `:133`, global delete `:136`, DoesNotExist `:138`);
  `PageLog` columns `db/models/page.py:80-110`.

## FX-LOG-01 process_logs

- `fx-log-01-process-logs.json` — `bgtasks/logger_task.py:22-36`
  (get_mongo_collection), `:38-60` (safe_decode_body, EXECUTED 7 vectors),
  `:62-89` (sinks), `:91-100` (routing on is_configured); `APIActivityLog`
  columns `db/models/api.py:97-126`.

## FX-EVT-01 track_event

- `fx-evt-01-track-event.json` — `bgtasks/event_tracking_task.py:24-41`
  (posthogConfiguration), `:43-59` (role matrix), `:61-81` (gate `:65-67`,
  capture args `:73-78`, False-on-error `:79-81`); event names
  `utils/analytics_events.py:6-8`.
