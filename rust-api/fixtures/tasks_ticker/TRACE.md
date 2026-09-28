# TRACE — D-10 tasks_ticker fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
How values were produced: no-DB units were executed under
`pi_dash.settings.test` with no database (`_rrule` is Django-free and ran
without setup); scanner SQL was captured as the executed statements via
`CaptureQueriesContext` while running the real `scan_due_*` tasks;
DB-backed rows/tasks ran against scratch database `pidash_ticker_fx`
(TCP postgres `127.0.0.1:5439`, migrated from this tree) with the clock
frozen (`django.utils.timezone.now` → `2026-09-28T12:00:00Z`), jitter
mocked to `0.0`, `.delay()` fan-out mocked to record ids, `on_commit`
eager, `drain_pod_by_id` stubbed, prompt template seeded via
`seed_default_template`, and a real ONLINE runner on the project default
pod. Dispatch (`dispatch_continuation_run` / `dispatch_scheduler_run`) is
real except where a case says mocked. Generators are throwaway
(`/tmp/fx_ticker_nodb.py`, `/tmp/fx_ticker_db.py`); every value below is
the recorded output of the named code.

OUT OF SCOPE (no fixtures): `bgtasks/loop.py` (D-03 owns it); the
orchestration callees `dispatch_continuation_run`,
`dispatch_scheduler_run`, `is_ticking_state`, `_active_run_for` /
`_latest_prior_run` (D-12 territory — treated as seam calls; only their
return values' effects on ticker/scheduler rows are recorded).
Reading material only: `tests/unit/bg_tasks/test_agent_ticker.py` (564),
`test_scheduler_task.py` (425), `test_rrule.py` (271),
`tests/unit/test_celery_schedule.py` (27).

## FX-TICKER-01 models

- `models/issue_agent_ticker.columns.json` —
  `db/models/issue_agent_ticker.py:75-176` (fields 84-165, `Meta`
  167-176, `iaticker_enabled_next_run_idx` 171-176); `__str__` 178-179.
  Full concrete column list incl. inherited `BaseModel` audit/pk cols.
- `models/scheduler.columns.json` — `db/models/scheduler.py:107-151`
  (`Scheduler`: `scheduler_unique_workspace_slug_when_active` 142-148),
  `:154-248` (`SchedulerBinding`: RRULE bundle 175-179, `outcome_mode`
  default 189-193, `next_run_at` 195, `last_run` SET_NULL 199-205,
  `last_error` 207, `actor`/`pod` SET_NULL 209-229,
  `scheduler_binding_unique_per_project_when_active` 236-241,
  `sched_binding_due_idx` 243-248).
- `models/enums.json` — `db/models/issue_agent_ticker.py:41-61`
  (`TickerDisarmReason`, 6 values); `db/models/scheduler.py:25-27`
  (`SchedulerSource`), `:30-45` (`OutcomeMode`), `:52-93`
  (`OUTCOME_MODE_DIRECTIVES` verbatim), `:96-104`
  (`outcome_mode_directive` unknown-mode fallback probe).
- `models/constants.json` — `db/models/issue_agent_ticker.py:35-38`
  (`DEFAULT_INTERVAL_SECONDS`, `DEFAULT_MAX_TICKS`, `INFINITE_MAX_TICKS`,
  `JITTER_FRACTION`); `db/models/scheduler.py:22`
  (`LAST_ERROR_MAX_LEN=1000`).
- `models/budget_logic.golden.json` —
  `db/models/issue_agent_ticker.py:200-227` (`pool_size`,
  `effective_max_ticks`, `wait_allowance`), `:229-241` (`remaining`,
  `cap_reached`), `:243-245` (`tick_count` alias), `:64-72`
  (`jitter_seconds`). Ran the real methods; only `pool_size` stubbed per
  vector (no project row without DB); jitter sampled with
  `random.seed(0)`.

## FX-TICKER-02 rrule goldens

- `rrule/cron_to_rrule.golden.json` — `bgtasks/_rrule.py:125-212`
  (FREQ picker 159-175, `*/N` INTERVAL 179-195, BY* clauses 196-210);
  field grammar `:67-122`; DOW `7→0` `:147-149`; DOM+DOW rejection
  `:151-157`. Vectors cover every FREQ path (incl. `0 */6 * * *` →
  `FREQ=DAILY;BYHOUR=0,6,12,18`, not `HOURLY;INTERVAL=6`), Sunday `0`
  vs `7`, and all bad-grammar rejections (error type + message).
- `rrule/validate.golden.json` — `bgtasks/_rrule.py:218-263`
  (empty ok `:230-231`; SECONDLY rejected `:256-259`).
- `rrule/next_fire.golden.json` — `bgtasks/_rrule.py:269-336`
  (single-shot ahead/past `:321-328`; rdates appended `:310-313`;
  exdates skipped `:314-317`; tzid informational `:290-293` — UTC and
  `America/New_York` give the same instant; `None` on parse error
  `:334-336`; naive-exdate quirk `:323` — see ported bugs).
- `rrule/occurrences.golden.json` — `bgtasks/_rrule.py:339-406`
  (lazy `xafter` loop `:386-393`; cap/`has_more` `:392-393`;
  single-shot window `:394-402`; naive-exdate quirk `:395`).

## FX-TICKER-03 scanner SQL + result rows

- `scanners/scan_due_tickers.sql` + `.rows.json` —
  `bgtasks/agent_ticker.py:57-69` (enabled + `next_run_at<=now` +
  pending-entry OR INFINITE pool OR `used<pool+granted+waited` — SQL
  mirror of `effective_max_ticks` — + `ORDER BY next_run_at`). Rows:
  T1 due admitted; T2 future excluded; T3 disabled excluded; T4 at-cap
  excluded; T5 spent-pool + free pending admitted; T6 granted cap
  admitted; T7 waited cap admitted; T8 infinite pool (`-1`, used=500)
  admitted. Fan-out order is `next_run_at` ascending.
- `scanners/scan_due_bindings.sql` + `.rows.json` —
  `bgtasks/scheduler.py:115-128` (enabled + scheduler `is_enabled` +
  both `deleted_at` NULL + project `deleted_at` NULL +
  `next_run_at<=now` OR NULL + `ORDER BY next_run_at`). Rows: B1 NULL
  due admitted; B2 future excluded; B3 disabled excluded; B4
  scheduler-disabled excluded; B5 soft-deleted-scheduler cascade window
  excluded; B6 past due admitted; B7 soft-deleted-project excluded.
  Fan-out order shows Postgres `ASC NULLS LAST` (B6 before B1).

## FX-TICKER-04 ticker-fire DB before/after

`ticker/fire.before_after.json` — `bgtasks/agent_ticker.py:77-306`.
Pool recorded per case (`before.pool`).

- `happy-tick` — claim `:97-105`, clock advance `:179-205`
  (`used+1`, `last_tick_at=now`, `next_run_at=now+10800+0`), dispatch
  `:253` (`trigger=tick`, `phase_kind=coding-task`).
- `recheck-missing` (`:104-105`), `recheck-disabled` (`:109-110`),
  `recheck-future` (`:112-113`) → `False`, row untouched.
- `cap-disarm-cap-hit` / `cap-disarm-pool-spent` — `:121-145`
  (`used>=cap` pre-claim disarm; `POOL_SPENT` iff `pending_entry`
  `:127-129`; pending flags cleared).
- `preclaim-active-run` (`:160-165`), `preclaim-no-prior-run`
  (`:166-171`) → `False`, `next_run_at` UNCHANGED, budget unspent.
- `clock-advance-cap-edge` — `:194-220` (`used` 9→10, immediate
  `CAP_HIT` disarm `:216-220`, still dispatches `True`).
- `free-claim-spent-pool` — `:119, :210-215` (free run on spent pool:
  `used` unspent, `POOL_SPENT` not `CAP_HIT`, `trigger=run_ai`).
- `user-disabled-disarm` — `:221-227` (fires `True`, then
  `USER_DISABLED` disarm).
- `dispatch-none-rollback` — `:254-296` (mocked dispatch `None`:
  `used`/`next_run_at`/`enabled`/`disarm_reason`/all four
  `pending_entry*` restored; `last_tick_at` NOT restored `:257-258`).

## FX-TICKER-05 scheduler-fire DB before/after

`scheduler/fire.before_after.json` — `bgtasks/scheduler.py:144-283`.

- `happy-claim-advance` — claim `:156-181`, `next_run_at` advance from
  RRULE bundle `:182, :198-205` (NULL → next minute), `last_error`
  cleared `:203-204`; pointer `:229-261` (`last_run` set QUEUED,
  `last_error` cleared).
- `last-run-in-flight-skip` — `:86-96`, `:50-62`
  (`NON_TERMINAL_STATUSES`, all 9), skip `:174-180` (`False`,
  `next_run_at` unchanged for every status).
- `bad-rrule-disable` — `:182-196` (`last_error` wording
  `invalid rrule: dtstart=… rrule=…`, `enabled=False`,
  `next_run_at=None`).
- `bad-rrule-truncation-1000` — `:188-190` (`last_error` exactly 1000
  chars).
- `failed-run-pointer` — `:235-251` (FAILED run still advances the
  clock; `last_error` = `prompt build failed: …` wording;
  dispatch mocked to return the FAILED run).
- `healthy-run-clears-error` — `:200-205, :244-248` (stale
  `last_error` cleared; dispatch mocked healthy).
- `dispatch-none-rollback` — `:263-283` (dispatch mocked
  `(None, reason)`: `next_run_at` restored to NULL, `last_error` =
  `dispatch failed: <reason>` wording).

## FX-TICKER-06 beat schedule goldens

- `beat.json` — `celery.py:108-112` (`scan-due-agent-tickers` →
  `pi_dash.bgtasks.agent_ticker.scan_due_tickers`,
  `crontab(minute=*)`), `:121-124` (`scan-due-scheduler-bindings` →
  `pi_dash.bgtasks.scheduler.scan_due_bindings`, `crontab(minute=*)`);
  `scan-due-loop-targets` (`:127-130`) present but EXCLUDED (D-03);
  all other 19 literal entries listed by name as EXCLUDED (other
  domains); settings-backed mechanism `:155-185` executed live (4
  entries, F-09); `tests/unit/test_celery_schedule.py:13-27`
  expectations executed (scheduled ⊆ registered tasks; legacy
  `github-issue-sync-every-4h` name kept).

## Cross-cutting ported bugs (translate as-is)

- BUG-TICKER-1: single-shot paths ignore naive exdates —
  `next_fire_from_rrule` (`_rrule.py:323`) and `occurrences_between`
  (`_rrule.py:395,400`) test membership with `in` on a tuple, and an
  aware `dtstart` never `==` a naive exdate, so a naive exdate at the
  exact firing instant does not exclude. (The recurring path is
  unaffected: dateutil normalizes naive rdates/exdates to UTC at
  `:310-316` / `:371-378`.) Pinned by the `quirk-single-shot-naive-*`
  vectors in `rrule/next_fire.golden.json` / `rrule/occurrences.golden.json`.
