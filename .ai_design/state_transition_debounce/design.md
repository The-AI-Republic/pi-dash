# State-Transition Settling Window — the hold on the pending entry

> Directory: `.ai_design/state_transition_debounce/`
>
> **Status:** implemented (PDASHOSS01-139, second attempt). Builds on
> `.ai_design/ticking_relevance/design.md` ("One Task, Three Stages, Ten
> Runs") — read §4.0 (one clock), §4.5 (entry-run queue) and §5.6
> (agent-vs-human moves) first; this design is a small extension of §4.5.
>
> Supersedes the first attempt (PR #370), which was written against the
> pre-`0163` three-clock model and introduced an `IssuePendingDispatch`
> side table, a token scheme and a dedicated Celery countdown task. See
> §5 for why that shape is not needed under the one-clock model.

## 1. Problem

A state change that targets a ticking state dispatches the entry run
immediately: `post_save(Issue)` → `handle_issue_state_transition` →
`reconcile` returns `dispatch_now` and the run is created synchronously in
the same request. Two failures, using Todo → In Progress → In Review:

1. **No settling window.** Dragging a card to In Progress starts a coding
   run instantly. A correction a few seconds later — a kanban mis-drop, or
   a deliberate two-step move — cannot take it back.
2. **Rapid moves waste runs.** Each ticking target fires (or queues) its
   own entry; the intermediate stop's run is wrong the moment the second
   move lands.

## 2. Design: the settling window is a hold on the pending entry

The ticker row already implements "a run is owed on this issue; fire it
when the issue is free" (`pending_entry`, design §4.5), and it already
persists everything a deferred dispatch needs:

- `pending_entry_free` — the agent-vs-human classification (§5.6),
  consumed from `moved_by_run` **at reconcile time**, so nothing about the
  moving run has to survive into a deferred payload;
- `pending_entry_actor` / `pending_entry_trigger` — who asked and how the
  fired run must be labelled;
- `next_run_at` — when the scanner may claim it.

So the settling window is one line of semantics: **a human transition's
entry run is queued on the clock with `next_run_at = now +
STATE_TRANSITION_SETTLE_SECONDS` (15 s) instead of dispatched inline.**

- **Coalescing** falls out of "one clock, one row": a further move inside
  the window re-enters `reconcile` on the same row and re-times the same
  pending entry to the _new_ stage — the clock is already "a queue of
  length one" (§4.5). Todo → In Progress → In Review ends as a single
  pending entry that fires the review template.
- **Cancellation** falls out of the existing exits: a move out of the
  bucket (`LEFT_BUCKET`) stops the clock and clears the pending flags; a
  cross-project move runs `want_run=False` → `_retime_clock` →
  `_clear_pending`; deletion soft-deletes the ticker row (and `fire_tick`
  additionally refuses a soft-deleted issue).
- **Firing** is the minute scanner (primary) plus a best-effort
  `fire_tick.apply_async(countdown=16)` accelerator scheduled on commit
  for ~15 s precision. There is no token: `fire_tick`'s claim already
  re-validates `enabled`, `next_run_at <= now`, the ticking state and the
  active-run guard under `select_for_update`, so a stale accelerator from
  a superseded move is a natural no-op — and a _lost_ accelerator is
  recovered by the scanner within a minute.

### 2.1 Who settles

| Path                                                                                                                     | Settles? | Why                                                                                                                 |
| ------------------------------------------------------------------------------------------------------------------------ | -------- | ------------------------------------------------------------------------------------------------------------------- |
| `post_save` signal, human move, real transition (`from_state` known)                                                     | **yes**  | the kanban drag / API patch this design exists for                                                                  |
| `post_save` signal, creation directly into a ticking state (`from_state is None`)                                        | no       | deliberate (a form, `issue create --state`), not a mis-drop; many helpers rely on immediate arming                  |
| Agent move (`moved_by_run` set, §5.6)                                                                                    | no       | already queues on the clock (counting, or parks on a spent pool); the run is active so the fire waits for it anyway |
| `dispatch_immediate=False` (`X-Pi-Dash-Skip-Immediate-Dispatch`, Comment & Run, cross-project move, Re-tick-from-Paused) | no       | the caller owns dispatch; the clock is only re-timed                                                                |
| Direct programmatic callers (`assistant/tools/issues.py`, `assistant/tools/runs.py`)                                     | no       | deliberate chat commands; they keep the synchronous `created_run` contract                                          |
| Run AI / Comment & Run / Re-tick / timer ticks                                                                           | no       | not transitions                                                                                                     |

The flag is threaded as `TickerEvent.settle`, set by
`handle_issue_state_transition(settle=...)`; only `signals.py` passes
`settle=from_state is not None`.

### 2.2 The first run

Before this change, an issue's _first_ run was always created inline by
the transition handler, so the queued-entry fire path
(`dispatch_continuation_run`) could safely skip issues with no prior run.
With the window, the first delegation is itself a pending entry, so a
**pending-entry claim may mint the first run** (`allow_first_run=True`
from `fire_tick`, fresh session, no parent). Plain timer ticks keep the
no-prior-run skip — a tick is a continuation, not an entry.

### 2.3 The bounce must not loop

The no-eligible-runner preflight now runs at fire time. When it bounces
the issue to Backlog (or, with no safe Backlog target, disarms the clock
in place), `fire_tick`'s post-dispatch claim rollback must not resurrect
`enabled` / `pending_entry` / `next_run_at` — that would re-fire the
entry and re-post the bounce comment every scanner pass. The rollback
restores only the budget (`used`) when the issue left the bucket or the
clock was disarmed with `LEFT_TICKING_STATE` underneath the claim.

## 3. What deliberately does not change

- **A room change with a run in flight does not cancel the run.** The
  original Part 2 wanted the in-flight run cancelled and a successor
  created from the terminal callback. Under the one-clock model the
  pending entry _is_ the successor: it fires as soon as the old run ends,
  with the finished workpad and hand-off in its prompt (§4.5 explains why
  early-created successor rows are wrong — `drain_pod` would hand them to
  a second idle runner, and the prompt would be built too early). The old
  run finishing on a stale prompt while the card already reads the next
  stage is a known, accepted gap — cancelling it is a separable change.
- **Non-ticking exit with a run in flight** stays option (a) (run keeps
  going, clock goes dormant) — decided on the first attempt and unchanged.
- **`used` / `granted`** are never touched by any transition through the
  window; `fire_tick`'s claim stays the only writer of `used`, and a
  human's settled entry is free (`pending_entry_free`).

## 4. Guarantees (acceptance criteria)

1. Todo → In Progress → In Review inside the window → exactly one run, on
   the review template (fresh session), firing after the last move
   settles (15 s hold + up-to-60 s scanner granularity; the accelerator
   makes it ~15 s).
2. Todo → In Progress → Todo inside the window → zero runs, clock dormant.
3. An agent-made move is still an agent move: the entry counts against
   the pool; a spent pool fires nothing. (`pending_entry_free=False` is
   written at reconcile time from `moved_by_run` — this is the regression
   that killed PR #370, where the signal rewiring dropped the argument.)
4. Comment & Run on a Paused issue is unchanged (`dispatch_immediate=False`
   path, no window).
5. `used` / `granted` survive every transition through the window.
6. A room change with a run in flight still owes exactly one entry run,
   fired once the issue is free, prompt built at fire time.
7. Single-active-run: unchanged — the window only ever _defers_ into the
   already-guarded `fire_tick` / `dispatch_continuation_run` path.

## 5. Why not the PR #370 shape (side table + token + countdown task)

|                | `IssuePendingDispatch` table (PR #370)                                                                                                | Hold on `pending_entry` (this design)                                             |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Schema         | new table + migration (collided with `0163`…`0167`)                                                                                   | none                                                                              |
| Coalescing     | monotonic token; stale job checks token                                                                                               | same row re-timed; stale fire fails the `next_run_at` check                       |
| Agent-vs-human | must thread `moved_by_run` into the stored intent (dropped → every agent move read as human)                                          | consumed at reconcile time, persisted as `pending_entry_free`                     |
| Delivery       | one Celery countdown task per transition is the _only_ fire path — a lost message strands the issue (flagged in PR #370's own review) | scanner is the fire path of record; the countdown task is an optional accelerator |
| Cancellation   | explicit deletes on issue delete / move                                                                                               | inherited from `LEFT_BUCKET` / `_clear_pending` / soft-delete cascade             |
| Fire precision | exactly 15 s                                                                                                                          | 15 s with the accelerator; 15–75 s if the accelerator is lost                     |

## 6. Touchpoints

- `orchestration/scheduling.py` — `STATE_TRANSITION_SETTLE_SECONDS`,
  `TickerEvent.settle`, `_queue_entry(not_before=)`, the settle branch in
  `_on_enter_or_move`, `_schedule_settle_fire`,
  `dispatch_continuation_run(allow_first_run=)`.
- `orchestration/service.py` — `handle_issue_state_transition(settle=)`;
  `entry-settling` outcome reason.
- `orchestration/signals.py` — passes `settle=from_state is not None`.
- `bgtasks/agent_ticker.py` — soft-deleted-issue guard; pending-entry
  claims may mint the first run; bounce-aware claim rollback.
- Tests: `tests/unit/orchestration/test_reconcile.py` (settling-window
  section), `tests/unit/runner/test_runs_views.py`
  (`test_no_skip_header_creates_run_on_state_change` updated to the
  queued contract).

## 7. Open / deferred

- Project-level override of the 15 s constant — deferred until someone
  asks; it is one column and one `getattr` away.
- Cancelling the in-flight run on a room change (or on Paused/Cancelled)
  — separable follow-up, see §3.
- Sub-minute pickup after the hold without the accelerator (e.g.
  `finalize_run_terminal` calling `fire_tick` directly) — §4.5 already
  lists it as a nice-to-have.
