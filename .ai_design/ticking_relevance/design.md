# Ticking Relevance — One Task, Three Stages, Ten Runs

> Directory: `.ai_design/ticking_relevance/`
>
> **Status:** implemented on branch `feat/ticking-relevance` (one PR, all
> four phases of §12). Two implementation notes that differ in detail from
> the text below: the yield endpoint lives in the external API
> (`POST /api/v1/workspaces/<slug>/agent-runs/<run_id>/yield/`, API-key
> auth — the `pidash` CLI authenticates as the runner owner, not as a
> runner), and the runner injects `PIDASH_RUN_ID` through a task-scoped
> `RUN_ENV` read by `util::shell::login_shell_command`, which every bridge
> already goes through, rather than by threading a parameter into each
> bridge's spawn signature.
>
> **Post-review amendments** (PR #387 code review): (1) a bridge's terminal
> payload is _merged_ into a yielded `done_payload`, never replacing it;
> (2) a run that failed / was cancelled without yielding keeps the clock
> ticking (only a _completed_ run gets the per-kind default; a paused one
> is `waiting_on_human`); (3) parking on a spent pool uses a distinct
> `pool_spent` disarm reason — `cap_hit` is reserved for the timer tick that
> consumed the last run and is the only reason that auto-Pauses, so the
> Re-tick the §5.4 comment points at stays reachable; (4) a queued human
> entry remembers who asked (`pending_entry_actor` / `_trigger`) and fires
> as them; (5) a human lever on a switched-off clock still fires its one
> run, then `fire_tick` re-applies the switch; (6) the yield endpoint and
> the run-id header require the run to be the caller's (creator, run
> owner, or runner owner) — membership alone is not authority; (7) a run id
> that is not active on _this_ issue makes the request a plain one rather
> than a 400 (the CLI sends the header on every write); (8) the cloud
> agent's `pidash_transition_current_issue` tool attributes its move to the
> run; (9) the human levers re-time the clock inside the dispatch
> transaction, so a click that produced no run leaves the ticker untouched;
> (10) migration `0163` folds prior Re-tick grants into `granted` and stamps
> `cap_hit` on rows already over the new pool.
> Builds on `.ai_design/issue_ticking_system/`,
> `.ai_design/create_review_state/`, `.ai_design/create_test_state/` and
> `.ai_design/ticking_optimization/` (fresh session per run, workpad as
> protocol). It does **not** replace the three-phase model; it makes the
> three phases aware of each other.
>
> **Scope:** In Progress, In Review and In Test stay three separate prompt
> kinds and three separate agent runs, each with a small stage goal. What
> changes: (1) every run knows the whole path to done and decides where the
> issue goes next — including backwards; (2) the workpad becomes the
> agent-to-next-agent channel across stage boundaries; (3) tick budgets are
> one pool per issue **for the life of the issue**, spent only by machine-started runs, and extended only by Re-tick;
> (4) the controller reads each run's outcome instead of guessing.
>
> **What this changes about today's code**
>
> - The ticker becomes **one clock per issue** that reads the current stage
>   as a parameter, never torn down and rebuilt on a stage change.
> - One budget **pool of 10 runs per issue**, any stage; replaces the three
>   per-stage caps (24 / 4 / 3). Re-tick grants +3. Entry runs count.
> - Only machine-started runs (timer ticks, agent-made moves) spend budget.
>   Human-started runs (a human moving the issue, Comment & Run, Run AI) are
>   free and always fire — one run each. Agent-made moves when the pool is
>   spent fire nothing. Re-tick is the only way to add budget.
> - A new locked section `task-lifecycle` joins all three recipes;
>   `state-routing`'s per-phase text collapses into it. `blocking` joins the
>   `review` and `test` recipes.
> - Review and test prompts inline the workpad and must write it before
>   exit. A `### Path to done` block is added to the workpad template.
> - The run outcome is reported through `pidash` and read by the ticker
>   (replaces the dead `pi-dash-done` fence / `done_payload["status"]` gate).

## 1. Problem

The ticking system treats In Progress, In Review and In Test as three
strangers. Each has its own clock, budget and prompt, and the ticker forgets
everything at a boundary. Observed consequences (all verified in code):

1. **Ticks are irrelevant to each other.** A review run's picture of the past
   is the comment thread plus the implementation run's final message.
   Review and test prompts never inline the workpad (`workpad_body` is
   rendered only by `workpad-setup`, which is coding-task only) and are never
   told to write it. What a review run learns dies with the run.
2. **Backwards is not a first-class move.** `review-cycle` sends "changes
   needed" to Blocked; `test-cycle` sends defects to Blocked. A human must
   notice and push the issue back to In Progress. The agent is not told that
   In Progress exists as a destination.
3. **Nothing reads the run's outcome.** The prompt says "emit noop"; the only
   consumer, `scheduling.maybe_disarm_on_terminal_signal`, gates on
   `run.done_payload["status"]`, which local runners never send (bridges emit
   `{"conclusion": …}`), and `orchestration/done_signal.py` has no callers.
   An approved review therefore ticks to its cap saying "no change".
4. **Re-entry refills the budget.** `arm_ticker` sets `tick_count = 0` on
   every disarm/re-arm (`orchestration/scheduling.py:148`). Combined with
   fix (2) this is a dead loop: Review → Progress → Review → … each with a
   fresh budget.
5. **Dangling prompt references.** `review`/`test` recipes omit `blocking`
   but four of their sections say "follow _Blocking the run_"; `ending-run`
   asks review/test runs to verify implementation-only workpad fields;
   review/test never see the repo/PR block from `intro`.

## 2. Goal

Every agent run on an issue should be able to answer, from what it is given:

- _What is this task's goal, and what does "finished" mean?_
- _Which stage is the task at, and how did it get here?_
- _What did the previous run leave for me?_
- _When I am done with my stage's work, where should the task go next — and
  will an agent actually run there?_

And the controller should be able to answer, from what the run reports:

- _Should I tick again, wait, or stop?_

Three runs, three small goals, one shared picture of the task. Not one big
loop.

## 3. Design principles

1. **Three prompts stay three prompts.** Stage-specific instructions
   (`implementation`, `review-cycle`, `test-cycle`) are unchanged in spirit.
2. **One lifecycle, shared.** A single locked section describes the path,
   exit conditions and allowed moves; every recipe renders it.
3. **Workpad is the agent's memory; comments are for humans.** If the next
   agent needs it → workpad. If a human needs it → comment. If the machine
   needs it → state + outcome.
4. **Only the machine spends budget.** The budget exists to stop the
   ticking system running away, so only runs the ticking system starts
   count against it. A human has to click, and a human cannot loop — their
   runs are free. Only Re-tick adds budget.
5. **The agent is told the truth about budget** before it chooses a next
   state, and must tell the human when it parks the issue somewhere no agent
   will follow. The budget never decides the state (§4.3).
6. **One clock per issue, never rebuilt.** The three stages are a _bucket_;
   moving between them is a parameter change to one ticker, not a teardown
   and re-arm. One `reconcile` function handles every event (§4.0, §10).

## 4. Lifecycle

### 4.0 The ticking bucket — one clock, three rooms

```
                 ┌───────────── ticking bucket ─────────────┐
 Backlog ──►     │  In Progress ◄──► In Review ◄──► In Test  │  ──► Done
 Todo    ──►     │                                           │  ──► Cancelled
                 │        one ticker, one budget pool        │  ──► Blocked / Paused
                 └───────────────────────────────────────────┘
```

**Inside the bucket there is one clock.** It does not matter which door the
issue came in through or how many times it walks between the three rooms —
same ticker row, same `used` counter, same `next_run_at`. Moving between
rooms changes only _which interval the clock reads_ and _which prompt the
run gets_. Today each move is a teardown and rebuild (`disarm_ticker` then
`arm_ticker`), which is where the counter reset, the lost entry run and the
wrong-clock disarm all come from.

**Outside the bucket the clock is dormant.** Not deleted — dormant. `used`
and `granted` are kept for the life of the issue; if the issue comes back
in, it picks up where it left off.

**Entry cases.**

| Move                                                | What happens                                                                                                                                                                                                                                                                          |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Backlog / Todo → In Progress                        | Enters through the front door. Human move → one free implementation run now. Clock arms on the In Progress interval (12 h) if the pool has budget.                                                                                                                                    |
| Backlog / Todo → In Review directly                 | Allowed — a human did the work and wants a review. Free review run now. There is no prior run and no workpad: the review reviews whatever the issue links to (a PR, a doc, the description) and _creates_ the `Path to done` block. `review-intro` gets one sentence for this (§8.3). |
| Backlog / Todo → In Test directly                   | Same shape; free test run; acceptance criteria come from the description (the test prompt already handles "criteria absent").                                                                                                                                                         |
| In Progress ↔ In Review ↔ In Test                   | Parameter change, no rebuild. See the event table.                                                                                                                                                                                                                                    |
| Any → Done / Cancelled / Backlog / Blocked / Paused | Leaves the bucket; clock dormant; counters kept. A human moving it back in fires a free run.                                                                                                                                                                                          |

**What the one clock does per event** (this is the contract `reconcile`
implements — §10):

| Event                                  | Ticker action                                                                                                                                                                                                                                                                           |
| -------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Issue enters the bucket (from outside) | Wake. Human move → free entry run now (§4.5 if a run is active). Arm the clock on the stage's interval if `used < cap`.                                                                                                                                                                 |
| Issue moves between rooms              | No rebuild. Agent move (§5.6) → if `used < cap`, queue the entry run (counts); if the pool is spent, fire nothing — the issue parks (§5.4). Human move → free entry run regardless. Re-time the clock to the new interval. Capture the implementation parent on every cross-stage move. |
| Timer due                              | `fire_tick`: read the _current_ stage at claim → prompt kind; `used += 1`; render fresh.                                                                                                                                                                                                |
| Run ends with an outcome (§7)          | If the issue is still in the stage the run was rendered for: `progressed` / `waiting_on_external` → next tick; `done` (stay) / `waiting_on_human` / `blocked` → clock stops. If the issue has already moved on: ignore — the clock is already set for the new room.                     |
| Pool spent (`used == cap`)             | Clock stops, `cap_hit`. In Progress additionally → Paused at run end (as today). Re-tick appears.                                                                                                                                                                                       |
| Re-tick                                | `granted += 3`; fire now (§4.5 if a run is active).                                                                                                                                                                                                                                     |
| Run AI / Comment & Run                 | One free run now; the clock re-times only if `used < cap`.                                                                                                                                                                                                                              |
| Issue leaves the bucket                | Dormant. Keep `used` / `granted`.                                                                                                                                                                                                                                                       |

### 4.1 The path

```
In Progress ──► In Review ──► In Test ──► Done
     ▲              │             │         (human only)
     └──────────────┴─────────────┘
          agent may send back with an open-items list
```

### 4.2 Exit conditions and allowed moves

| Stage            | Finished when                                                                                              | Next state chosen by the run                                                                                    |
| ---------------- | ---------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| In Progress      | change made and validated; PR open (or non-code answer posted); acceptance criteria written to the workpad | → In Review                                                                                                     |
| In Review        | no unresolved findings against the work product                                                            | → In Test (clean) · → In Progress (real defects, listed) · stay (waiting on a human reviewer / nothing changed) |
| In Test          | every acceptance criterion verified from the user's side                                                   | stay (pass — human closes) · → In Progress (defects, listed)                                                    |
| Done / Cancelled | —                                                                                                          | a human decides. The agent never moves an issue to Done.                                                        |

**Blocked** is reserved for "I need a human" (missing auth, or a decision only
a human can make). A spent pool is **not** a reason to go to Blocked — the
issue goes to its truthful state and the comment says no run will follow
(§5.4). It is no longer the destination for "the code
has a bug".

### 4.3 The next-state decision (every run, every stage)

The last step of every run is the same:

1. State the stage result in one line (`approved`, `2 defects`, `3/3
criteria verified`, `nothing changed`).
2. Pick the next state from the table in §4.2.
3. Check the budget line (§5.3). If the pool is **spent**, still choose
   the truthful destination, but post the mandatory human comment (§5.4).
   **The budget never changes which state you choose** — the state records
   what is true about the work; the budget changes only what you tell the
   human. A review with one run left that "approves" a PR with a known
   defect because sending it back "wouldn't do anything" has made the state
   a lie. Where the budget _may_ shape behaviour is how the last run spends
   itself: leave the task hand-off-able rather than start something it
   cannot finish.
4. Write `### Path to done` on the workpad (§6) — including _why_ this state.
5. Move the issue (or leave it) via `pidash issue patch`.
6. Report the outcome (§7).

### 4.4 Agent-made moves fire the next stage's entry run — as soon as the issue is free

When a run moves an issue to another stage — forward (In Progress → In
Review) or backward (In Review → In Progress with open items) — the entry
run for the new stage should start as soon as the current run ends, **if
the pool has budget left**. The context is fresh and the run just wrote
a precise hand-off; waiting one interval wastes it.

**Today this does not happen, in either direction.** The agent moves the
issue from _inside_ its run, so `handle_issue_state_transition` finds an
active run and creates nothing (`orchestration/service.py:182-186`); the new
stage's ticker is armed for `now + interval`, and nothing at run end
re-dispatches. An implementation run that moves an issue to In Review today
gets its review ~8 h later. §4.5 fixes this.

The implementation parent is captured on _every_ cross-stage move, not only
`started → *` (today `service.py:154` only stashes `resume_parent_run` when
leaving `started`). The reverse hand-back after a test run must still find
the implementation lineage.

### 4.5 Entry-run queue: the ticker is the queue

**The situation.** An implementation run is working on PIDASH-42. Near the
end it runs `pidash issue patch PIDASH-42 --state "In Review"`. We want a
review run to start right after this implementation run finishes.

**Why we can't just create the review run right there.** The obvious idea is
to create the review `AgentRun` row with status `QUEUED` at the moment the
state changes, and let it start when the implementation run ends. Two things
go wrong:

1. _The runners would start it right away, not "when the implementation run
   ends"._ Pi Dash hands queued runs to runners through `drain_pod`
   (`runner/services/matcher.py`). That function asks "which runners are
   idle?", not "which issues are busy?". If there is a second idle runner in
   the pod, it grabs the queued review run while the implementation run is
   still going — two agents on the same branch at once. This is exactly why
   today's code refuses to create the row while a run is active.
2. _The review run's prompt would be built too early._ A run's prompt is
   written when the row is created, and it inlines the comment thread and
   the workpad. A review row created while the implementation run is still
   going would not contain the hand-off that run writes in its last minutes
   — the acceptance criteria, the PR link, the final workpad. The review run
   would start blind.

**What we do instead: let the ticker be the queue.** The ticker already has
both properties we need. `fire_tick` (the per-minute scanner worker)
_refuses to fire while a run is active_ and _builds the prompt at the moment
it fires_. So the sequence becomes:

1. The state changes to In Review while the implementation run is still
   active.
2. Instead of refusing, `reconcile` marks the next tick for this issue as
   due **now** (`next_run_at = now`, not `now + interval`) and sets
   `pending_entry = true` on the ticker row — "a first run for this stage is
   owed".
3. Every minute the scanner sees the ticker is due. It checks: is a run
   active? Yes → skip, leave `next_run_at` alone, try again next minute.
   (This is existing behaviour.)
4. The implementation run ends.
5. Within the next minute the scanner sees the ticker is due and no run is
   active → it fires. The review prompt is built _now_, with the finished
   workpad and hand-off in it. `pending_entry` is cleared. It counts
   against the pool (`used += 1`), and goes through the existing
   claim/rollback safety.

That is the whole mechanism. "A queue of length one" just means an issue can
only ever be waiting for _one_ next run, and the ticker row is where that
wait is recorded. If the issue moves again before the pending run fires, the
pending entry simply now belongs to the newer stage — it is the same clock.

**Two extras.**

- _Pickup delay._ Because the scanner runs once a minute, the review run
  starts up to 60 s after the implementation run ends. If we want it
  instant, the code that finalizes a run (`finalize_run_terminal`, which
  already re-fires the pod drain on commit) can call `fire_tick` for that
  issue directly when `pending_entry` is set. Nice-to-have, not required.
- _Re-tick, Comment & Run, and human moves._ These also mean "start a run
  now", and have the same problem if a run happens to be active. They use
  the same trick: mark the ticker due now, set `pending_entry`, let it fire
  when the issue is free. Because human-started runs are free (§5.2), the
  flag records its origin — `pending_entry_free = true` — and `fire_tick`
  skips the counter increment on that claim.

**What the human sees.** While `pending_entry` is true the ticker card shows
"next run queued", so the review is visibly coming, not stuck.

## 5. Budget

### 5.1 One pool

|                | Value                                  | Notes                                                                |
| -------------- | -------------------------------------- | -------------------------------------------------------------------- |
| Pool per issue | **10** machine-started runs, any stage | `Project.agent_default_max_ticks`; replaces the three per-stage caps |
| Re-tick grant  | **+3**                                 | `Project.agent_retick_grant`, new                                    |
| Intervals      | 12 h / 8 h / 12 h per stage            | unchanged — cadence is rhythm, not budget                            |

The per-stage split (4 / 3 / 3) considered earlier is **dropped**. It was a
consequence of three separate clocks; with one clock it is a leftover, and
it can strand an issue — In Progress spent while Review and Test still hold
5 unused runs — for a reason that has nothing to do with runaway loops. If
a per-stage share is ever wanted again, it is an independent improvement
design, not part of this one.

### 5.2 Rules

- **One counter, `used`, for the life of the issue.** It never resets on a
  stage change or on re-entry to the bucket.
- **Machine-started runs count**: timer ticks, and the entry run fired by
  an agent-made move (§4.5). Whatever stage they run in.
- **Human-started runs are free**: a human moving the issue into the
  bucket or between stages, Comment & Run, Run AI. Each fires exactly one
  run, always — even when the pool is spent — and does not touch `used`. A
  human cannot loop, so there is nothing to guard against.
- **There is no refill.** To give the _clock_ back, a human presses Re-tick
  (§5.5), which adds `granted += 3`. Cap = project default + `granted`.
- **At cap**, the clock stops with `cap_hit`. In Progress additionally →
  Paused at run end (as today); In Review / In Test stay put. Re-tick
  appears.
- **Worst case with no human:** 10 runs, then silence. No bounce rule is
  needed; the pool _is_ the fence.

### 5.3 The agent sees the budget

Every prompt (all three kinds) carries one line, rendered from the ticker
— including when the clock is stopped:

```
Runs used on this issue: 7 of 10 (3 remaining). When this reaches 10, no
agent run will follow any state move you make — a human must act.
```

This replaces the single-phase `tick` block in `session-framing`.

### 5.4 Parking an issue when the pool is spent

A run that can see it is the last one in the pool, or that moves the issue
after the pool is spent:

- still moves the issue to the truthful state (§4.3 — the state says where
  the task _is_);
- posts a comment addressed to the human: what it found, that no agent run
  will follow, and the ways forward — fix it by hand, press **Run AI** /
  reply with **Comment & Run** for one free run, or press **Re-tick** to
  give the clock back;
- reports outcome `waiting_on_human` (§7).

The clock stays stopped with `cap_hit`. Nothing fires.

### 5.5 Human re-ticking

Principle: **human-started runs are free and always fire; only Re-tick adds
budget.**

| Human lever                      | Today                                                                                                                                             | New                                                                                                                                                               |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Re-tick** (`re_tick_ticker`)   | only when the current state is spent; adds one phase budget to that state's cap, keeps the cumulative count, re-arms; next run after the interval | only when the pool is spent; `granted += 3`, cumulative display ("10 of 13"); **fires a run now** (via §4.5 if a run is active). The only lever that adds budget. |
| **Comment & Run**                | zeroes `tick_count`, fires now                                                                                                                    | fires now, **free** (no count, no reset). Re-arms the clock only if the pool has budget.                                                                          |
| **Human moves the issue**        | enters state, counter → 0, entry run fires                                                                                                        | entry run fires, **free**, even when the pool is spent. `used` untouched. Clock re-armed only if budget remains. Distinguished from an agent move by §5.6.        |
| **Run AI**                       | fires one run, ticker untouched                                                                                                                   | unchanged — free                                                                                                                                                  |
| **Disable ticking on the issue** | on/off                                                                                                                                            | unchanged. Per-issue cap overrides are gone (§9); Re-tick is the only per-issue budget lever.                                                                     |

Worked example — the pool is 10 of 10 and a review run sends the issue
back with two defects:

- Agent move → pool spent → no run; the issue parks In Progress and the
  review run's comment says so (§5.4).
- Human presses **Run AI** → one free run fixes the defects and moves the
  issue to In Review; the clock is still stopped (pool spent), so the
  review waits for a human or another free run. `used` still reads 10 of
  10 — nothing was refilled, nothing needed to be.
- Or the human presses **Re-tick** → 10 of 13, clock re-armed, run fires
  now, and the next three machine runs can carry the issue through review
  and test on their own.

### 5.6 Telling agent moves from human moves

Whether an entry run fires when the pool is spent, and whether it counts,
depends on knowing — at the moment the issue changes state — whether an
**agent run** or a **human** made the move (§5.2). Two things
stand in the way today:

1. _The information never reaches the ticker._ State changes are handled by
   a Django `post_save` signal that fires on any issue save and calls
   `handle_issue_state_transition(..., actor=None)`
   (`orchestration/signals.py:74`). Nothing tells it who saved.
2. _Account identity is the wrong signal._ When the agent runs
   `pidash issue patch`, the CLI authenticates with the runner's token,
   which resolves to the **runner's owner** — a human account
   (`runner/authentication.py`, `api/middleware/api_authentication.py`).
   The patch view records that human as the actor
   (`api/views/issue.py:746-758`). An `is_bot` check would say "human" for
   every agent move and refill every budget.

So the question is not _who_ but **"was this move made from inside an agent
run?"** — and that is easy to carry:

1. The runner injects `PIDASH_RUN_ID` into the agent process, next to
   `PIDASH_ISSUE_IDENTIFIER` and `PIDASH_PROJECT_ID` (§10, runner).
2. Every mutating `pidash` route sends it as a header,
   `X-Pi-Dash-Run-Id: <uuid>`.
3. The work-item patch view verifies the header — the run exists, is
   active, is for this issue, and belongs to the authenticated runner — and
   sets `issue._orchestration_moved_by_run = run` before saving, the same
   trick already used for `_orchestration_dispatch_immediate`.
4. The `post_save` hook reads the attribute and passes
   `moved_by_run` into `handle_issue_state_transition`.
5. `reconcile`: `moved_by_run` set → agent move: if the pool is spent,
   fire nothing; otherwise queue the entry run and it counts. Not set →
   human move: fire the entry run, free; re-arm the clock only if budget
   remains.

A human clicking in the UI never has a run id, so the check cannot be fooled
by account identity. The same header is what `pidash run yield` uses (§7).

## 6. Workpad as the cross-stage channel

Each run is a fresh session. The workpad is the only memory that survives,
so it must carry what the _next_ run — in **any** stage — needs.

### 6.1 `### Path to done` block (new, owned by all stages)

Added to `workpad-template`, placed first after the location fence:

```md
### Path to done

- **Goal**: <one line — what "finished" means for this issue>
- **Stage**: In Review
- **History**: In Progress (3 runs) → In Review (1) → In Progress (1) → In Review
- **Next state**: In Progress — because: 2 defects (see Open items)
- **Open items**:
  - [ ] null check in `export_csv` (review finding #1)
  - [ ] acceptance criterion 3 not yet verified
- **Acceptance criteria**: (canonical copy — the hand-off comment is the human-readable mirror)
  - [ ] Criterion 1
  - [ ] Criterion 2
```

Rules:

- Every run in every stage **reads the workpad first, writes it last**.
- `Open items` is the hand-off list. A review run that finds defects writes
  them here; the next In Progress run starts from this list, not from the
  comment thread.
- `History` is appended by the run that makes the move.
- `workpad-template` says "re-write the full body every time". For this
  block that means **carry it forward and edit it**: an In Progress run
  _consumes_ `Open items` (checks them off, removes what is done) and
  _appends_ to `History`; it never regenerates the block from scratch or
  drops items it did not address.
- The existing implementation sections (`Phase`, `Progress Checkpoints`,
  `Analysis`, `Plan`, `Validation`) are **owned by In Progress**. Review and
  test runs leave them alone.

### 6.2 What each stage writes

| Stage       | Writes                                                                                                              |
| ----------- | ------------------------------------------------------------------------------------------------------------------- |
| In Progress | everything it does today, plus `Path to done` (goal, stage, acceptance criteria, next state)                        |
| In Review   | `Path to done`: stage, history, next state + reason, open items (findings not auto-fixed)                           |
| In Test     | `Path to done`: stage, history, next state + reason, open items (failed criteria / defects), per-criterion verdicts |

### 6.3 Comments shrink to their real job

Comments carry: the run's human-readable summary, questions the agent cannot
answer alone, the spent-budget notice (§5.4), and the acceptance-criteria
hand-off as a _mirror_ for humans. They are never the agent's memory.

## 7. Outcome reporting (fixes the unread-noop bug)

**The situation.** A run ends. The ticker has to decide: tick again, wait
for a human, or stop. Today it cannot — the prompt says "emit noop", the only
consumer of that (`maybe_disarm_on_terminal_signal`) reads
`run.done_payload["status"]`, local runners never send a `status` key
(bridges emit `{"conclusion": …}`), and the old `pi-dash-done` fence parser
has no callers. So every review ticks to its cap saying "no change".

**The channel.** The agent's only write path is `pidash`, so the outcome
travels the same way, as the run's last act:

```
pidash run yield --outcome <progressed|waiting_on_human|waiting_on_external|done|blocked> [--note "<one line>"]
```

**Which run?** The cloud must know _which_ AgentRun is reporting. The CLI
reads `PIDASH_RUN_ID` from the environment (§5.6 step 1) and sends it as
`X-Pi-Dash-Run-Id`. The endpoint verifies the run exists, is active, belongs
to the authenticated runner, and is for the issue named in the request;
then writes `run.done_payload = {"status": <outcome>, "note": …,
"yielded_at": …}`. A yield with no run id, a stale run id, or another
runner's run id is rejected — the outcome must be attributable or it is
worthless.

| Outcome               | Meaning                                                                                                 | Ticker effect                                                                                                                                             |
| --------------------- | ------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `progressed`          | did work, more to do                                                                                    | keep ticking (`next_run_at = now + interval`)                                                                                                             |
| `waiting_on_human`    | posted a question / spent-budget notice                                                                 | disarm (`terminal_signal`); re-arm on human action (§5.5)                                                                                                 |
| `waiting_on_external` | In Progress waiting on CI or a merge                                                                    | keep ticking (heartbeat)                                                                                                                                  |
| `done`                | this stage's exit condition is met — the issue was moved on, **or** it is approved / verified and stays | stop the clock — **unless** the issue has already moved on, in which case the clock is already set for the new stage and must be left alone (guard below) |
| `blocked`             | cannot proceed                                                                                          | disarm; issue → Blocked per `blocking`                                                                                                                    |

**The guard: `done` must not stop a clock that has moved on.** A run that
moves the issue forward (step 5 of §4.3) and _then_ yields `done` (step 6)
would, with today's hook, stop the issue's clock — which one second earlier
was re-timed for the **next** stage and (with §4.5) holds the queued entry
run. `reconcile` therefore compares the stage the run was rendered for with the
issue's _current_ stage. The run needs a `phase_kind` stamp at creation:
`prompt_manifest` carries the kind only for Cloud Agent runs (`{"v": 2,
"kind": …}`); for local runs it is a bare list of section entries, so it
cannot be used for this.

- same stage → apply the outcome to the ticker as in the table;
- different stage → the run already handed the issue on; **do nothing** to
  the ticker.

This makes the yield/move order irrelevant to correctness, so the agent
does not have to remember one.

**Defaults.** A run that exits without yielding is treated per kind:
`coding-task` → `progressed` (keep ticking; the budget still bounds it);
`review` / `test` → `done` (stay; disarm). The review/test default is the
cost-safe one — an approved review that forgets to yield must not tick to
its cap as it does today.

`noop` is retired as a vocabulary word; "nothing changed" is `done` (stay)
for review/test and `progressed` with no changes for In Progress.
`waiting_on_external` is for In Progress only; a review or test waiting on
a _person_ (a reviewer, the human who closes to Done) is `done` (stay).

## 8. Prompt changes

### 8.1 New locked section `task-lifecycle` — in all three recipes

Contents: the path diagram (§4.1), the exit-condition / allowed-moves table
(§4.2), the next-state decision steps (§4.3), the spent-state rule (§5.4),
and the budget line (§5.3, rendered). Replaces the phase-specific bullets of
`state-routing` for `started` / `review` / `test`; `state-routing` keeps only
the `backlog` / `unstarted` / `completed` / `cancelled` no-op routing.

### 8.2 Recipe membership

```
coding-task: intro, session-framing, pidash-cli, task-lifecycle, default-posture,
             autonomy, state-routing, analyze-and-scope, workpad-setup,
             implementation, blocking, guardrails, workpad-template, ending-run
review:      review-intro, session-framing, pidash-cli, task-lifecycle,
             repo-context*, workpad-context*, review-cycle, blocking, guardrails, ending-run
test:        test-intro, session-framing, pidash-cli, task-lifecycle,
             repo-context*, workpad-context*, test-cycle, blocking, guardrails, ending-run
```

`*` — two small locked sections split out of `intro` / `workpad-setup` so
review and test get the repo/PR block and the inlined `workpad_body` without
the implementation-only text around them.

### 8.3 Edits to existing sections

- `session-framing`: the `{% if tick %}` block becomes the one-line budget
  (§5.3), rendered even when the clock is stopped; the `(emit noop)`
  parenthetical goes.
- `review-intro` / `test-intro`: one sentence for direct entry (§4.0) — "if
  there is no prior run, the work product is what the issue links to; create
  the `Path to done` block yourself."
- `review-cycle` Step 3 / `test-cycle` Step 3: outcomes become the §4.2
  moves; "changes needed → Blocked" becomes "→ In Progress with open items".
- `ending-run`: the workpad checklist is guarded — implementation fields for
  `coding-task`, `Path to done` for `review` / `test`. The "Analyze & scope"
  tone reference is guarded the same way. Adds the `pidash run yield` step.
- `implementation` Step 7: acceptance criteria are written to the workpad
  `Path to done` block _and_ mirrored in the hand-off comment.
- `workpad-template`: adds `### Path to done`.

## 9. Schema

`IssueAgentTicker` — one clock per issue, never rebuilt:

```
next_run_at, pending_entry, pending_entry_free, disarm_reason, user_disabled
used       IntegerField(default=0)   machine-started runs, any stage
granted    IntegerField(default=0)   added by Re-tick
resume_parent_run                    kept; captured on every cross-stage move
```

- `tick_count` → renamed `used` (migration: rename; values carry over).
- `max_ticks`, `review_interval_seconds`, `review_max_ticks`,
  `test_interval_seconds`, `test_max_ticks`, `interval_seconds` — **dropped**.
  Budget policy lives on the project; only consumption lives on the issue.
  (Per-issue interval overrides go unless someone asks for them back.)
- `enabled` becomes **derived**: `not user_disabled and project ticking
enabled and used < cap and disarm_reason not in {terminal_signal}` — the
  scanner, serializer and `can_re_tick` read the derived value.
- `pending_entry`, `pending_entry_free` — §4.5.

`Project`:

- `agent_default_max_ticks` 24 → **10** (now the pool).
- `agent_retick_grant` new, default **3**.
- `agent_review_default_max_ticks`, `agent_test_default_max_ticks` —
  dropped. The three `*_interval_seconds` stay.

`orchestration/agent_phases.py`: `CADENCE_FIELDS` shrinks to interval only;
`cap`/`count` resolution goes through the pool.

`AgentRun`: `phase_kind` (`CharField`, the recipe kind the run was rendered
for — §7 guard). `done_payload` already exists.

## 10. Code touchpoints

### 10.1 `orchestration/scheduling.py` — one mutator

Today six functions mutate the ticker row from their own angle
(`arm_ticker`, `disarm_ticker`, `reset_ticker_after_comment_and_run`,
`re_tick_ticker`, `maybe_disarm_on_terminal_signal`,
`maybe_apply_deferred_pause`). They become thin senders of one event each
into a single function:

```
reconcile(issue, event) -> TickerDecision

events:
  entered_bucket(moved_by_run | None)
  moved_stage(from, to, moved_by_run | None)
  left_bucket
  run_ended(run, outcome | None)        # outcome from done_payload; None → per-kind default
  human_run_requested(kind)             # run_ai | comment_and_run
  retick
  tick_due                              # from fire_tick, after the claim
```

`reconcile` reads: current stage, `used`, `granted`, project pool + grant +
interval for the stage, `user_disabled`, project ticking enabled, whether a
run is active. It writes: `next_run_at`, `pending_entry`,
`pending_entry_free`, `disarm_reason`, `granted`, `resume_parent_run`. It
never writes `used` — only `fire_tick`'s claim does. The event table in §4.0
is its specification; the guard in §7 is one branch of `run_ended`.

Keep the old function names as one-line wrappers for a release so callers
and tests migrate gradually.

### 10.2 Everything else

- `orchestration/service.py`: `handle_issue_state_transition` classifies
  the move (enter / between rooms / leave), captures `resume_parent_run` on
  every cross-stage move, and sends the event; the `active-run-exists`
  branch no longer bails — it lets `reconcile` queue the entry (§4.5).
- `orchestration/signals.py`: forward `issue._orchestration_moved_by_run`
  as `moved_by_run` (§5.6).
- `bgtasks/agent_ticker.py`: scan admits `used < project.agent_default_max_ticks
  - granted`(one comparison replaces the per-group`Case/When`);
`fire_tick`reads the current stage at claim → prompt kind,`used += 1`unless`pending_entry_free`, clears both pending flags.
- `runner/services/run_lifecycle.py`: `finalize_run_terminal` /
  `apply_run_paused` send `run_ended(run, outcome)`; optionally call
  `fire_tick` directly when `pending_entry` is set (sub-minute pickup).
- `runner/views/run_endpoints.py`: new `POST /runs/<run_id>/yield/`
  (`pidash run yield`); verifies run ↔ runner ↔ issue ↔ active; writes
  `done_payload`.
- Work-item patch view (`api/views/issue.py`): read `X-Pi-Dash-Run-Id`,
  verify, set `issue._orchestration_moved_by_run` before save (§5.6).
- Runner (`runner/src/daemon/supervisor.rs`): inject `PIDASH_RUN_ID` next to
  `PIDASH_ISSUE_IDENTIFIER`.
- `pidash` CLI (`runner/src/cli/`): `run yield` subcommand; send
  `X-Pi-Dash-Run-Id` from `PIDASH_RUN_ID` on every mutating route.
- `orchestration/agent_phases.py`: `CADENCE_FIELDS` keeps interval only;
  cap resolution goes through the pool; `auto_pause_on_cap` stays per stage.
- `prompting/context.py`: `_tick_context` returns `used`, `cap`,
  `remaining`, renders even when the clock is stopped; `workpad_body` and
  `repo` / `code_reviews` rendered by the two new small sections (§8.2).
- `prompting/recipes.py`, `prompting/seed.py`, `prompting/sections/*.md`,
  `prompting/validation.py` sample context.
- `app/serializers/issue.py`: ticker card fields `used`, `cap`,
  `pending_entry`; `can_re_tick` = in bucket and `used >= cap`.
- Web: ticker card shows `used of cap`, "next run queued", Re-tick on a
  spent pool.

## 11. Open questions

1. ~~Comment & Run: refill or grant?~~ Resolved: neither. Human-started
   runs are free and never touch counters (§5.2); only Re-tick grants.
2. ~~Does a human move into a spent state refill it?~~ Resolved: it fires
   one free run and refills nothing. "Human" means "no run id on the move"
   — §5.6.
3. `waiting_on_external` — heartbeat only for now; GitHub webhooks (review /
   CI / merge → wake) are out of scope for this design.
4. Should In Review's "stay — waiting on a human reviewer" keep ticking
   (spending budget) or disarm? Design: `done` (stay) → disarm; a human
   reviewer's comment re-arms via Comment & Run.

## 12. PR sequence

1. **Outcome channel.** `PIDASH_RUN_ID` in the agent env, `pidash run
yield` + endpoint, `done_payload` write, disarm hook with the stage-match
   guard and per-kind defaults. Independently valuable: stops today's noop
   ticks.
2. **One clock, one pool.** `reconcile` replaces the six mutators; `used`
   / `granted`; drop the per-stage cap columns; entry-run queue (§4.5);
   agent-vs-human via `X-Pi-Dash-Run-Id` (§5.6); parent captured on every
   cross-stage move. Independently valuable: hard ceiling of 10, no
   counter reset, no lost entry run, review starts right after
   implementation.
3. **Prompt: `task-lifecycle` + recipe fixes.** Lifecycle section, `blocking`
   / repo / workpad in review & test, budget table, guarded `ending-run`,
   `Path to done` in the template.
4. **UI:** `used of cap`, "next run queued", Re-tick on a spent pool.

Each PR is shippable alone; 1 and 2 are the safety fixes and go first.

## 13. Tests

Unit (`tests/unit/orchestration/`, `tests/unit/bg_tasks/`):

- `reconcile` table tests — one case per row of the §4.0 event table, plus:
  agent move with pool spent → nothing fires, issue state still changes;
  human move with pool spent → free run, `used` unchanged; `done` after a
  forward move → clock untouched (§7 guard); `done` in place → clock stops;
  no outcome → per-kind default.
- `fire_tick`: claim while a run is active leaves `next_run_at` and `used`
  untouched; `pending_entry_free` claim does not increment `used`; prompt
  kind resolved from the state _at claim time_, not at arm time.
- Scan SQL: admits `used < pool + granted`, `-1` unlimited.
- Re-tick: `granted += 3`, fires now, only when `used >= cap`.
- Migration: `tick_count` → `used` carries values; dropped columns gone.
- Yield endpoint: rejects missing / stale / foreign run id; writes
  `done_payload.status`.
- Patch view: header present + valid → `moved_by_run` set; absent → not.

Prompt (`tests/unit/prompting/`): all three recipes render with
`task-lifecycle`, `blocking`, the budget line (stopped clock included);
review/test render `workpad_body` and `code_reviews`; no dangling
"Blocking the run" / "Analyze & scope" reference in review/test output.

Runner (`runner/tests/`): `PIDASH_RUN_ID` present in the agent env;
`pidash run yield` sends the header; mutating routes send the header.

## Appendix A — Worked timeline (the bounce that needs no bounce rule)

Pool 10, intervals 12 h / 8 h / 12 h. Human delegates PIDASH-42.

| #   | Event                                 | Stage after | `used` | Clock                                                                |
| --- | ------------------------------------- | ----------- | ------ | -------------------------------------------------------------------- |
| 1   | Human: Todo → In Progress             | In Progress | 0      | free run now; next tick +12 h                                        |
| 2   | Run: PR opened, → In Review, `done`   | In Review   | 0      | guard: moved on → pending entry now                                  |
| 3   | Tick (entry)                          | In Review   | 1      | review run                                                           |
| 4   | Run: 2 defects, → In Progress, `done` | In Progress | 1      | pending entry now                                                    |
| 5   | Tick (entry)                          | In Progress | 2      | fixes, → In Review, `done`                                           |
| 6   | Tick (entry)                          | In Review   | 3      | approved, → In Test, `done`                                          |
| 7   | Tick (entry)                          | In Test     | 4      | criterion 3 fails, → In Progress                                     |
| 8   | Tick (entry)                          | In Progress | 5      | fix, → In Review                                                     |
| 9   | Tick (entry)                          | In Review   | 6      | approved, → In Test                                                  |
| 10  | Tick (entry)                          | In Test     | 7      | all pass, stays, `done`                                              |
| —   |                                       | In Test     | 7      | clock stopped (`done` in place); 3 runs unused; human closes to Done |

Pathological variant — the fix at step 5 never satisfies review: steps
4–5 repeat. `used` reaches 10 at the fifth round trip; the run that sees
`used == 10` parks the issue in its truthful state and posts the §5.4
comment. Silence until a human presses Re-tick, Run AI, or replies. No
bounce counter was needed.
