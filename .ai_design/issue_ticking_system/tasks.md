# Issue Ticking System — Implementation Tasks

This file turns the design into a concrete implementation checklist.

Related docs:

- `design.md`

## Suggested rollout

### PR A — Schema and state seeding

Goal:

- land the new schema, project fields, and Paused state without changing runtime behavior

Scope:

- add `IssueAgentSchedule` model in `apps/api/pi_dash/db/models/issue_agent_schedule.py` (fields per design §7.1)
- add three fields to `Project`: `agent_default_interval_seconds`, `agent_default_max_ticks`, `agent_ticking_enabled`
- migration M1: `CreateModel(IssueAgentSchedule)` + `AddField × 3` on `Project`, plus the `(enabled, next_run_at)` index
- update `apps/api/pi_dash/seeds/data/states.json` to include a `Paused` entry in the `backlog` group
- migration M2: `RunPython` data migration backfilling a `Paused` state in every existing project that lacks one (idempotent)
- migration M3: `RunPython` data migration backfilling `IssueAgentSchedule` rows for every issue currently in the literal "In Progress" state, with `next_run_at = NOW() + project_default_interval + jitter`
- register `IssueAgentSchedule` in Django admin (read-only is fine; primarily for ops)

Why first:

- everything downstream depends on the schema landing
- zero behavior change in this PR (no scanner, no auto-trigger removal), so it's safe to land independently

### PR B — Orchestration scheduling primitives

Goal:

- create the public functions that the scanner, Comment & Run, and state-transition paths will all use

Scope:

- create `apps/api/pi_dash/orchestration/scheduling.py` with:
  - `arm_schedule(issue, *, dispatch_immediate=True)`
  - `disarm_schedule(issue)`
  - `reset_schedule_after_comment_and_run(issue)`
  - `dispatch_continuation_run(issue, *, triggered_by)`
  - `maybe_apply_deferred_pause(run)`
- add `effective_interval()` and `effective_max_ticks()` model methods on `IssueAgentSchedule`
- add `jitter(interval_seconds)` helper
- wire `arm_schedule` into `handle_issue_state_transition` after the existing immediate-dispatch path
- wire `disarm_schedule` into the same handler for transitions out of Started
- tests per design §12.3

Why second:

- with primitives in place, both the still-live comment-auto-trigger path and the new tick path can call into them
- comment auto-trigger and terminate sweep are still active in this PR — they just coexist with the new schedule rows. Behavior is unchanged.

### PR C — Scanner and auto-trigger removal

Goal:

- start the periodic ticking; remove the comment auto-trigger and the terminate-side comment sweep

Scope:

- create `apps/api/pi_dash/bgtasks/agent_schedule.py` with:
  - `scan_due_schedules` (Celery beat target; fans out `fire_tick` per due row)
  - `fire_tick(sched_id)` (atomic claim + dispatch per design §6.1, including the literal "In Progress" name check)
- add `CELERY_BEAT_SCHEDULE` entry: `scan-due-agent-schedules` runs `crontab(minute="*")`
- verify Beat is deployed as a singleton (check `docker-compose-local.yml` and beat entrypoint)
- remove `orchestration/signals.py:fire_comment_continuation` (the `post_save(IssueComment)` receiver)
- remove `orchestration/service.py:maybe_continue_after_terminate`
- replace the `transaction.on_commit(...)` calls to `maybe_continue_after_terminate` at `runner/consumers.py:550` (`_handle_run_paused`) and `:610` (`_finalize_run`) with calls to `maybe_apply_deferred_pause(run)`
- delete or convert tests per design §12.3
- add new test modules: `tests/unit/orchestration/test_scheduling.py`, `tests/unit/bgtasks/test_agent_schedule.py`

Why third:

- after this PR ships, comments are inert by default and ticking is live
- this is the first PR that materially changes user-facing behavior

### PR D — UI surfaces

Goal:

- expose ticking settings and status to users; enforce inert-comment behavior in the composer

Scope:

- comment composer (issue detail page): verify PR #62's "Comment" and "Comment & Run" buttons match the design; default Enter posts only — must NOT fire a run
- "Next agent check: in 2h 14m" status row near the composer, driven by `next_run_at`
- cap-hit copy in red: "Agent has stopped polling — issue moved to Paused after N ticks. Click Comment & Run or move back to In Progress to resume."
- Paused-issue confirmation dialog when Comment & Run is clicked on a Paused issue (per design §4.6); Cancel aborts the whole action; Confirm runs the three-step client flow
- "no cap" label + running tick count when `max_ticks = -1`
- project create / edit page: new "AI agent ticking" section with three controls (enabled, default cadence, max ticks)
- issue settings: per-issue overrides for the same three fields (cadence, max ticks, "disable ticking for this issue")
- server: route `POST /api/runners/runs/` through `dispatch_continuation_run(issue, triggered_by="comment_and_run")` + `reset_schedule_after_comment_and_run(issue)` in one transaction
- server: when the state-transition view is invoked as part of a Comment & Run flow on a Paused issue, arm with `dispatch_immediate=False` so Comment & Run owns the single dispatch

Why fourth:

- backend is fully done by PR C; this is the user-visible surface
- can be split further (composer vs. settings vs. cap-hit indicator) if reviewable size requires it

### PR E (optional) — Prompt-system updates

Goal:

- teach the agent it can be re-invoked periodically and should prefer cheap exits

Scope:

- update prompt fragments in `apps/api/pi_dash/prompting/templates/` so the agent:
  - emits `noop` when nothing has changed since the last run (don't fabricate a `paused` question)
  - emits `paused` only when there's a genuine question for the human
  - emits `completed` / `blocked` per existing semantics
- update tests for the modified prompt rendering

Why optional:

- not a hard blocker; the agent already produces sensible output. This is a tightening pass that can ship after PR C/D.

## Detailed task lists by area

### Schema + migrations

- [ ] `apps/api/pi_dash/db/models/issue_agent_schedule.py` (new file): model class with fields per design §7.1
- [ ] `apps/api/pi_dash/db/models/__init__.py`: export the new model
- [ ] M1 migration: `CreateModel(IssueAgentSchedule)` + `AddField × 3` on `Project` + `Index(fields=['enabled', 'next_run_at'])`
- [ ] Update `apps/api/pi_dash/seeds/data/states.json`: add `Paused` entry, `"group": "backlog"`, distinct color and sequence
- [ ] M2 migration: `RunPython` backfilling `Paused` state for every existing project that lacks one. Idempotent — skip projects that already have it.
- [ ] M3 migration: `RunPython` backfilling `IssueAgentSchedule` rows for issues currently in literal "In Progress" state. `next_run_at = NOW() + project_default_interval + jitter`, `tick_count = 0`, `enabled = NOT user_disabled`.
- [ ] Verify all three migrations are reversible (M1 drops the model + fields; M2/M3 delete the inserted rows)

### Orchestration scheduling primitives — `orchestration/scheduling.py`

- [ ] `arm_schedule(issue, *, dispatch_immediate=True)` — idempotent; honors `user_disabled` and project `agent_ticking_enabled`; sets `enabled = false` when either suppresses ticks
- [ ] `disarm_schedule(issue)` — idempotent; sets `enabled = false`
- [ ] `reset_schedule_after_comment_and_run(issue)` — uses `select_for_update` to serialize against `fire_tick`; resets `tick_count = 0` and `next_run_at = NOW() + interval + jitter`
- [ ] `dispatch_continuation_run(issue, *, triggered_by)` — resolves parent (latest prior run), creator (system bot for ticks; comment author for Comment & Run), pod; calls `_create_continuation_run`; returns the created run or `None` when blocked by `_active_run_for`
- [ ] `maybe_apply_deferred_pause(run)` — implements design §4.4.1: when schedule disarmed AND issue still in Started AND no other active runs, transition issue In Progress → Paused with the `pi_dash_agent` bot as actor
- [ ] `IssueAgentSchedule.effective_interval()` and `effective_max_ticks()` model methods returning override-or-project-default
- [ ] `jitter(interval_seconds)` helper — uniform `random(0, interval × 0.1)`

### Wiring into existing orchestration

- [ ] `handle_issue_state_transition` (`orchestration/service.py:88`): after the immediate-dispatch path, call `arm_schedule(issue, dispatch_immediate=True)`
- [ ] `handle_issue_state_transition`: on transitions out of Started, call `disarm_schedule(issue)`
- [ ] When the Comment & Run flow drives a Paused → In Progress transition, the view should arm with `dispatch_immediate=False` so Comment & Run owns the single dispatch

### Background tasks — `bgtasks/agent_schedule.py`

- [ ] `scan_due_schedules` Celery Beat task: query enabled, due, under-cap rows; fan out `fire_tick.delay(sched_id)`
- [ ] `fire_tick(sched_id)` Celery worker task: open transaction, `select_for_update` the row, re-check, advance `tick_count` + `next_run_at`, run In Progress name check, dispatch via `dispatch_continuation_run(issue, triggered_by="tick")`
- [ ] Add `CELERY_BEAT_SCHEDULE` entry running `scan_due_schedules` every minute
- [ ] Verify Beat is deployed as a singleton

### Removal of auto-trigger

- [ ] Remove `orchestration/signals.py:fire_comment_continuation` (post_save IssueComment receiver)
- [ ] Remove `orchestration/service.py:maybe_continue_after_terminate`
- [ ] Replace `runner/consumers.py:550` and `:610` `transaction.on_commit(...)` calls to `maybe_continue_after_terminate` with `maybe_apply_deferred_pause(run)`

### Comment & Run server flow

- [ ] Server handler for `POST /api/runners/runs/` routes through `dispatch_continuation_run(issue, triggered_by="comment_and_run")` + `reset_schedule_after_comment_and_run(issue)` in a single transaction
- [ ] State-transition view detects "this transition is part of a Comment & Run on a Paused issue" (header / query flag / explicit endpoint) and arms with `dispatch_immediate=False`

### UI (apps/web)

- [ ] Plain "Comment" button (default Enter): post comment only — must NOT call `/api/runners/runs/`
- [ ] "Comment & Run" button: post comment, then call `/api/runners/runs/`
- [ ] On a Paused issue, "Comment & Run" shows confirmation dialog before any side effects; Cancel aborts the whole action (including the comment); Confirm runs the three-step client flow (post comment → PATCH state → POST run)
- [ ] "Next agent check: in 2h 14m" status row driven by `next_run_at`
- [ ] Red "not ticking" indicator + cap-hit copy when schedule is disarmed and issue is Paused
- [ ] "no cap" label when `max_ticks = -1`
- [ ] Project create / edit: AI agent ticking section (enabled, default cadence picker, max ticks)
- [ ] Issue settings: per-issue cadence override, max ticks override, "disable ticking for this issue" toggle

## Tests

Per design §12.3.

### Convert (still test the function, just call it explicitly)

In `apps/api/pi_dash/tests/unit/orchestration/test_service.py`:

- [ ] `test_comment_creates_pinned_continuation` (line 253) — call `handle_issue_comment` directly
- [ ] `test_comment_from_bot_is_ignored` (line 277)
- [ ] `test_comment_on_backlog_issue_is_ignored` (line 291)
- [ ] `test_comment_with_no_prior_run_skipped` (line 302)

### Delete

- [ ] `test_comment_during_active_run_held_for_terminate_sweep` (line 313) — sweep is removed; replace with a tick-side equivalent in `test_agent_schedule.py`
- [ ] `test_two_comments_coalesce_into_one_followup` (line 338) — comments no longer create QUEUED follow-ups on their own
- [ ] `test_terminate_sweep_picks_up_held_comment` (line 378) — sweep removed
- [ ] Any test asserting on `orchestration_error_count` from the `fire_comment_continuation` signal receiver (the receiver is gone)

### New — `tests/unit/orchestration/test_scheduling.py`

- [ ] `arm_schedule` honors `user_disabled` and project `agent_ticking_enabled`
- [ ] `arm_schedule(dispatch_immediate=False)` does not call into run dispatch
- [ ] `disarm_schedule` is idempotent
- [ ] `reset_schedule_after_comment_and_run` resets `tick_count` and `next_run_at`; serializes against concurrent `fire_tick`
- [ ] `dispatch_continuation_run` resolves parent, creator, pod correctly and returns `None` when blocked by `_active_run_for`
- [ ] `maybe_apply_deferred_pause` only transitions when (schedule disarmed) AND (no active runs) AND (issue still in Started)

### New — `tests/unit/bgtasks/test_agent_schedule.py`

- [ ] `scan_due_schedules` selects only enabled, due, under-cap rows
- [ ] `fire_tick` re-checks under lock and skips when conditions changed
- [ ] `fire_tick` increments `tick_count` and advances `next_run_at`
- [ ] `fire_tick` sets `enabled = false` on cap hit but does NOT auto-transition state (deferred pause)
- [ ] `fire_tick` honors the literal "In Progress" name check (skips workspaces with custom Started state names)
- [ ] `fire_tick` skips when an active run exists; does not consume cap budget on skip
- [ ] Concurrency: two `fire_tick` calls on the same `sched_id` produce only one dispatch

### Migration tests

- [ ] M2 idempotent across re-runs
- [ ] M3 only backfills "In Progress" issues, not Backlog/Done/Paused
- [ ] M3 sets `next_run_at = NOW() + interval`, not `NOW()` (no deploy stampede)

### UI / integration

- [ ] Plain Enter posts comment but does not fire a run
- [ ] Comment & Run on In Progress fires a run + resets schedule
- [ ] Comment & Run on Paused shows confirmation dialog; on Confirm transitions state then fires
- [ ] Cap-hit indicator shows when schedule is disarmed and `tick_count >= max_ticks`

## Open follow-ups after MVP

- Generalize ticking to "any Started-group state," not just literal "In Progress" (Q4 in design §10).
- Quiet hours / business-hours awareness — additional gate inside `fire_tick` (Q5).
- Distinct system actor user separate from `pi_dash_agent` if the activity feed needs to differentiate workpad updates from system-driven state transitions (Q3).
- Optional `triggered_by` (`state_transition` / `tick` / `comment_and_run`) field on `AgentRun` for analytics (deferred per design §7.4).
- "Real progress" detection beyond the agent's own done-signal — file-diff or PR-event introspection. Explicitly out of scope for v1.
- Operational dashboard: "schedules firing in the next hour," cap-hit count, average tick_count per project, etc.
- Per-issue ticking history visualization in the issue detail page.
- Backoff cadence (e.g., 3h → 6h → 12h after consecutive `noop` ticks) instead of the simple total-count cap.
