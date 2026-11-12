# TRACE — D-03 loop auto-pm fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
How values were produced: pure-Python units were executed under
`pi_dash.settings.test` with no DB; SQL was captured with
`CaptureQueriesContext` plus the canonical `str(queryset.query)` compiler form;
DB-backed rows/tasks/handlers ran against scratch database `pidash_loop_fx`
(local socket postgres, migrated from this tree) with an in-memory KMS
stand-in for BYOK crypto. Generators are throwaway (`/tmp/fxA_nodb.py`,
`/tmp/fxB_db.py`); every value below is the recorded output of the named code.
Celery ran eager (`task_always_eager`) so `.delay()` cascades execute inline;
`run_assistant_turn` was mocked at `pi_dash.loop.dispatch` with call counts
recorded. DRF views were driven via `APIRequestFactory` + `force_authenticate`
(real view code, no Django test client).

Reading material only (no fixtures; assistant-domain code owned elsewhere):
`tests/contract/loop/test_runtime_seam.py`, `test_github_tool.py`,
`test_thread_visibility.py`.

## FX-LOOP-01 models

- `models/loop_job.columns.json` — `db/models/loop.py:41-81` (`Meta` 67-78,
  `loop_job_unique_slug_when_active` 72-78); audit columns `db/mixins.py:16-19`
  (`TimeAuditModel`), `:26-38` (`UserAuditModel`), `:57-69`
  (`SoftDeleteModel`); pk `db/models/base.py:17-21`.
- `models/loop_target.columns.json` — `db/models/loop.py:84-140` (`Meta`
  125-137, `loop_target_unique_edge_when_active` 130-136,
  `loop_target_due_idx` 137); FK `on_delete`/targets per lines 92-118.
- `models/loop_user_preference.columns.json` — `db/models/loop.py:143-182`
  (`Meta` 163-179; user+job partial unique 168-173, user master
  `job__isnull` partial unique 174-178; NULL job = master switch 153-154).
- `models/skip_reason.json` — `db/models/loop.py:25-39` (7 values + labels).

## FX-LOOP-02 serializer goldens

- `serializers/interval_label.golden.json` — `loop/serializers.py:16-32` (all
  six FREQ labels; unknown/empty → "periodically" :31-32; note the
  case-sensitive `FREQ=` prefix at :30 — `freq=daily` falls through).
- `serializers/public_job_payload.golden.json` — `loop/serializers.py:35-43`
  (5-key whitelist; `name` = `public_name`).
- `handlers/admin_job_payload.golden.json` — `loop/admin_views.py:43-59`
  (14-key full shape; `stats` appended by detail GET :135-149).
- `serializers/builtin_catalog.golden.json` — `loop/builtins.py:17-27`
  (dataclass), `:29-39` (prompt), `:46-59` (catalog: slug `auto-close-merged`,
  `min_role=15`, `rrule="FREQ=DAILY;BYHOUR=3;BYMINUTE=0"`); seeded
  `enabled=False` by `db/migrations/0149_loop_mvp.py:16,44`.

## FX-LOOP-03 query records

- `queries/due_targets.sql` + `.rows.json` — `loop/eligibility.py:88-100`
  (enabled + non-deleted job, `next_run_at <= now OR NULL`).
- `queries/eligible_due_targets.sql` + `.rows.json` — `loop/eligibility.py:103-114`
  with the 4 annotated Exists (`_member` :54-63, `_job_off` :66-74, `_paused`
  :77-85, `_llm` :38-42 via `_usable_llm_filter` :27-35); SCANNER block is the
  exact scanner slice `bgtasks/loop.py:163-165` (`order_by("next_run_at")`,
  `values_list("id")`). Compiler doubles `deleted_at IS NULL` (default manager
  + explicit filter) — port the SQL semantics as emitted.
- `queries/settings_payload_reads.sql` + `.rows.json` — `loop/views.py:24-49`.
- `queries/admin_stats_24h.sql` + `.rows.json` — `loop/admin_views.py:136-148`
  (`target_count`/`completed`/`failed`/`skipped`; `TurnStatus`
  `assistant/models.py:64-70`).
- `queries/targets_list.sql` + `.rows.json` — `loop/admin_views.py:183-210`
  (`select_related` :185, `skip_reason` :188-190, `workspace` :191-193, `status`
  :194-196, `page`/`per=50` :198-204).

## FX-LOOP-04 guard goldens

- `guards/read_enabled.golden.json` — `loop/views.py:52-61` (toggle-only:
  exactly `{"enabled": bool}`, else 400 `invalid_payload`).
- `guards/validate_writes.golden.json` — `loop/admin_views.py:27-40`
  (`_SLUG_RE`, `_VALID_ROLES={5,15,20}`, `_WRITABLE`), `:62-69` (hourly floor),
  `:72-105` (bad slug → `invalid_slug`, bad role → `invalid_min_role`, empty →
  `invalid_rrule` + "rrule is required", dateutil failures → `invalid_rrule` +
  detail, sub-hourly → `rrule_too_frequent`, create `missing_fields` with
  **sorted** detail :98-104, unknown keys dropped :75-78); rrule grammar
  `bgtasks/_rrule.py:218-263`.
- `guards/hourly_floor.golden.json` — `loop/admin_views.py:62-69`.
- `guards/slug_taken.golden.json` — `loop/admin_views.py:119-120` (create
  409), `:159-162` (patch self-exclusion).
- `guards/instance_admin_permission.golden.json` —
  `license/api/permissions/instance.py:12-18` (anonymous False, `role__gte=15`
  + instance scoping), applied `loop/admin_views.py:109,128,177`.

## FX-LOOP-05 task DB before/after

- `tasks/reconcile.before_after.json` — `bgtasks/loop.py:62-108` (minute
  throttle :69-71, per-job next fire :75, edge anti-join :80-93,
  `bulk_create(ignore_conflicts)` :104); new cursors land after now
  (next fire + stagger).
- `tasks/stagger.golden.json` — `bgtasks/loop.py:44-49` (`crc32(job:ws:user) %
  window`, window `LOOP_STAGGER_WINDOW_MINUTES` default 60).
- `tasks/advance_ineligible.before_after.json` — `bgtasks/loop.py:111-145`
  (per-job next-fire cache :129-131, `eligibility.check` :132, `bulk_update`
  :140-144); skip reason `llm_config_missing`, cursor past now.
- `tasks/fire.before_after.json` — `bgtasks/loop.py:184-226` (SFU claim
  :196-201, future-cursor race :206-207, cursor advance :210-213, recheck
  :215-222, dispatch :226); turn queued once, `user_message` = job prompt
  verbatim, target `thread.kind="loop"`.
- `tasks/fire_future_noop.json` — `bgtasks/loop.py:204-207` (False, no-op).
- `tasks/dispatch_turn_active.json` — `loop/dispatch.py:89-100` (in-flight
  turn → `TURN_ACTIVE`, no second turn) via `bgtasks/loop.py:226`.
- `tasks/dispatch_rotation.json` — `loop/dispatch.py:39-71` (threshold =
  `MAX_THREAD_MESSAGES` (`assistant/errors.py:106` = 200) −
  `LOOP_ROTATION_HEADROOM` (30) = 170; fresh title = `job.public_name[:255]`
  :64; old thread archived :68).
- `tasks/dispatch_error.json` — `loop/dispatch.py:122-131` (unexpected error →
  `DISPATCH_ERROR`, False).

## FX-LOOP-06 handler goldens

User surface (`loop/views.py:64-99`, routes `loop/urls.py:9-12`):

- `handlers/user_settings.golden.json` — settings GET (200 full payload),
  master PATCH (200 + preference row), bad type / extra key (400
  `invalid_payload`), job PATCH (200 toggled card), unknown slug (404
  `not_found`), bad payload on job PATCH (400 `invalid_payload`).

Admin surface (`loop/admin_views.py:108-234`, routes `loop/admin_urls.py:13-18`
under `/api/instances/loop/`):

- `handlers/admin_crud.golden.json` — non-admin GET 403; list 200 (full job
  incl. prompt); create 201 (`is_builtin=False`, `dtstart` defaulted to now
  :121-122); duplicate slug 409 `slug_taken`; bad slug 400 `invalid_slug`;
  sub-hourly 400 `rrule_too_frequent`; missing fields 400 `missing_fields`;
  detail 200 (+ `stats`); detail 404; `is_builtin` immutable on PATCH;
  PATCH slug clash 409; PATCH 404; DELETE 204 (soft delete, tombstone kept;
  re-DELETE 404 `not_found`).
- `handlers/admin_targets.golden.json` — targets 200 (`page` + `results`
  rows); unknown job 404; `skip_reason`/`workspace`/`status` filters; target
  row shape `admin_views.py:212-234` incl. `last_run.usage.total_tokens`.

## Cross-cutting ported bugs (translate as-is)

- BUG-LOOP-1: non-numeric `min_role` raises uncaught `ValueError`
  (`loop/admin_views.py:82` `int(...)`) → generic 500. Verified by running
  `_validate_writes` (see `guards/validate_writes.golden.json` raises vector).
- BUG-LOOP-2: `BaseAPIView.dispatch` returns `exc`, not `response`, after
  `handle_exception` (`app/views/base.py:151-163`; duplicated :211-262) —
  latent for exceptions escaping DRF's inner dispatch.
- Quirk (not a bug): `interval_label`'s `FREQ=` prefix match is case-sensitive
  (`loop/serializers.py:30`) while the value is uppercased (:31).
- Quirk: admin PATCH performs no type checking (`{"enabled": "x"}` passes
  validation; see `guards/validate_writes.golden.json` partial vectors).
- Env note (not ported): without a reachable broker, DELETE's cascade
  `soft_delete_related_objects.delay` (`db/mixins.py:78`) fails and the view
  500s; with a broker/worker it is 204 as recorded (celery ran eager here).
