# State-transition Debounce + Phase-change Supersede

> Directory: `.ai_design/state_transition_debounce/`
>
> **Status:** implemented (PDASHOSS01-139).
>
> **Scope:** two changes to how an Issue state change drives an agent run.
>
> 1. **15s debounce** — a transition that targets a _ticking_ state no
>    longer dispatches inline. It records the intent and fires after a 15s
>    quiet period; a further transition inside the window replaces the
>    intent and restarts the clock, so a rapid `Todo → In Progress → In
Review` mis-drop coalesces into a single dispatch on the final state.
> 2. **Phase-change supersede** — a phase change _after_ a run is already
>    in flight cancels that run and hands off to the new phase's run once
>    the old run terminates, mirroring the existing cross-project move
>    handoff.

Related designs: `.ai_design/issue_ticking_system/` (the per-issue
ticker), `.ai_design/create_review_state/` (phase registry, fresh-session
entry, terminal-signal disarm).

## 1. Problem

`orchestration/signals.py` fires `post_save(Issue)` →
`orchestration/service.handle_issue_state_transition`, which armed the
ticker and dispatched a run **synchronously, in the same request**. Two
failures followed, both visible on `Todo → In Progress → In Review`:

1. **No settling window.** Dragging a card to In Progress started a coding
   run instantly. A correction to In Review seconds later could not stop
   the already-queued coding run.
2. **The second transition was swallowed.** On `In Progress → In Review`
   the ticker was re-armed on the review cadence and `resume_parent_run`
   was set, but the inline dispatch hit the single-active-run guardrail
   (`service.py`) and the DB constraint `agent_run_one_active_per_work_item`
   — so **no review run was created**. The impl run kept running on the
   coding prompt while the card read In Review, and when it terminated its
   done-payload disarmed the freshly review-armed ticker
   (`maybe_disarm_on_terminal_signal`), parking the issue with no agent
   scheduled at all.

## 2. Design

### 2.1 Where the debounce lives

The debounce is layered in the **signal**, not in
`handle_issue_state_transition`. That function stays the synchronous
dispatcher used directly by tests, by the debounce job, by Comment & Run
(`dispatch_immediate=False`), and by the project-move handoff. The signal's
`dispatch_immediate=True` path now routes through
`service.route_state_transition`.

`route_state_transition` decides:

| condition                                                    | action                                                                                                       |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `from_state is None` (issue just created / prior state gone) | dispatch inline (no transition to settle)                                                                    |
| active run + cross-phase ticking change                      | **supersede** (Part 2)                                                                                       |
| active run, otherwise                                        | inline `handle_issue_state_transition` (a debounce would only no-op on active-run; arm/disarm still resolve) |
| target non-ticking, no active run                            | clear pending + inline disarm (settling-window cancel; bounce-to-Backlog)                                    |
| target ticking, no active run                                | **debounce** (Part 1)                                                                                        |

Issue **creation** into a ticking state is _not_ debounced (`from_state is
None`): there is no mis-drop to correct and many call sites rely on
immediate arming.

### 2.2 Part 1 — the debounce

New side table `IssuePendingDispatch` (one row per issue):

```python
token       BigIntegerField   # monotonic; bumped on every transition
dispatch_at DateTimeField      # when the armed job should fire (advisory)
from_state  FK(State, null)    # source of the latest transition
to_state    FK(State, null)    # target of the latest transition (advisory)
```

- **Schedule** (`_schedule_pending_dispatch`): bump `token`, set
  `dispatch_at = now + DEBOUNCE_SECONDS`, record `from_state` / `to_state`,
  and `transaction.on_commit` enqueue
  `bgtasks.state_transition_debounce.dispatch_debounced_transition(issue_id,
token)` with `countdown=DEBOUNCE_SECONDS`.
- **Cancel** (`_clear_pending_dispatch`): bump `token`, clear `dispatch_at`.
  Any already-scheduled job now carries a stale token.
- **Fire** (`run_debounced_dispatch`): re-read issue + pending row under a
  row lock; no-op on `issue-gone` (deletion / cross-project move bumped the
  token) or `stale-token`; else consume the row (bump token, clear
  `dispatch_at`) and call `handle_issue_state_transition(from_state=<stored>,
to_state=<current issue state>)`. The stored `from_state` preserves the
  cross-phase session-shape decision; the current state is authoritative.

`DEBOUNCE_SECONDS = 15`, a module constant in `orchestration.service`. A
project-level override is a possible future refinement; it is deliberately
out of scope here.

Because the whole transition body runs at fire time against the final
state, ticker arm/disarm resolve once, on the settled state — intermediate
arms never churn `tick_count` / `next_run_at`.

### 2.3 Part 2 — phase-change supersede

Mirrors the cross-project move handoff exactly (`utils/issue_move.py` +
`service._create_project_move_handoff_run` /
`complete_project_move_handoff`). New config key
`PHASE_CHANGE_HANDOFF_CONFIG_KEY = "_phase_change_handoff"`.

`supersede_active_run` (called from `route_state_transition` when an active
run exists and the transition crosses to a different ticking phase):

1. Disarm the old phase's ticker, arm the new phase's ticker (cadence
   resolves against the target state), capture `resume_parent_run` on the
   `started → review/test` forward transition.
2. Stash the marker on the run: `target_state_id`, `target_group`,
   `fresh_session`, `parent_run_id` (the successor's session shape —
   `fresh_session` for In Review / In Test, resume-parent for the review →
   In Progress hand-back).
3. **Inert** run (QUEUED / PAUSED, no runner): cancel and create the
   successor in the same transaction. **Executing** run: set
   `CANCEL_REQUESTED`, send a `cancel` frame after commit
   (`reason=issue_phase_changed`); the successor is created by
   `complete_phase_change_handoff` once the runner acknowledges and the run
   goes terminal.

`complete_phase_change_handoff` (called from the terminal callback) locks
issue → run, is idempotent via `replacement_run_id`, suppresses itself if
the issue moved phase again, and reuses `_create_and_dispatch_run` so the
successor renders the target phase's template with the resolved session
shape.

### 2.4 Suppressing the terminal-signal disarm

`agent_run_finalization.apply_terminal_effects` already skipped
`_apply_post_run_orchestration` (and the failure comment) when
`_has_project_move_handoff(run)`. That guard is extended to
`_has_phase_change_handoff(run)`; `pending_handoff` now carries the kind
(`"project_move"` / `"phase_change"`) and routes to the matching completer.
This keeps the cancelled old run's `completed` / `blocked` done-payload from
disarming the newly-armed phase ticker. The reap
(`session_service`) and runner-revoke (`runner/models`) recovery paths call
both completers (each is idempotent).

## 3. Invariants preserved

- **Single-active-run** and `agent_run_one_active_per_work_item`: the
  successor is only created after the old run reaches a terminal status
  (inert runs are cancelled first, in the same transaction), so the slot is
  never double-occupied.
- **`X-Pi-Dash-Skip-Immediate-Dispatch: 1`** (Comment & Run): still routes
  `dispatch_immediate=False` → `handle_issue_state_transition`, never
  debounced.
- **Bounce to Backlog** on no-eligible-runner: Backlog is non-ticking, so
  `route_state_transition` clears any pending dispatch and disarms — it
  cannot re-enter the debounce and loop.
- **Deletion / cross-project move inside the window**: the delayed job
  no-ops (`issue-gone` / `stale-token`).

## 4. Open question (deferred to a human decision)

What should happen when the new state is **non-ticking** (Paused / Done /
Backlog / Cancelled) and a run is in flight? Options: (a) leave the run
running — today's behavior; (b) cancel on any exit from the spawning phase;
(c) cancel only on Paused / Cancelled. The rest of Part 2 does not depend on
this. **Implemented as (a)** for now (the run continues, the ticker
disarms, no handoff marker) and flagged for a product decision; switching to
(b) or (c) is a localized change in the "target non-ticking, active run"
branch of `route_state_transition`.

## 5. Touchpoints

- `db/models/issue_pending_dispatch.py` + migration `0163` — the side table.
- `orchestration/service.py` — `route_state_transition`,
  `_schedule_pending_dispatch`, `_clear_pending_dispatch`,
  `run_debounced_dispatch`, `supersede_active_run`,
  `_create_phase_change_successor_run`, `complete_phase_change_handoff`,
  `_send_phase_change_cancel`, `PHASE_CHANGE_HANDOFF_CONFIG_KEY`,
  `DEBOUNCE_SECONDS`.
- `orchestration/signals.py` — route `dispatch_immediate=True` through
  `route_state_transition`.
- `bgtasks/state_transition_debounce.py` — the delayed worker.
- `runner/services/agent_run_finalization.py`,
  `runner/services/run_lifecycle.py` — extend the handoff guard.
- `runner/services/session_service.py`, `runner/models.py` — belt-and-braces
  handoff completion on reap / revoke.
- Tests: `tests/unit/orchestration/test_service.py`,
  `tests/unit/runner/test_runs_views.py`,
  `tests/unit/runner/test_issue_move_run_repoint.py`.
