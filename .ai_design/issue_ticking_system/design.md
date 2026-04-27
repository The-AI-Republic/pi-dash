# Issue Ticking System — Periodic Agent Re-invocation

> Directory: `.ai_design/issue_ticking_system/`
>
> **Status:** discussion / pre-implementation. No code changes yet.
>
> **Scope:** how an issue in **In Progress** gets the AI agent
> re-invoked on a periodic schedule so the agent gets multiple
> opportunities to make progress, with the agent self-deciding each
> turn whether to act or back off.
>
> **Related work**
>
> - `.ai_design/issue_run_improve/design.md` — multi-run continuation,
>   `PAUSED_AWAITING_INPUT`, runner pinning, drain triggers, comment
>   auto-trigger. This doc **supersedes the comment auto-trigger
>   sections** of that one (§5.2 of that doc and the
>   `maybe_continue_after_terminate` sweep). The PAUSED + pinning +
>   native-resume infrastructure carries over unchanged — only the
>   _what wakes the agent_ changes.

## 1. Problem

Today, transitioning an issue to **In Progress** creates exactly one
`AgentRun`. If the agent terminates without finishing — most commonly
emitting `paused` with a question for the human — nothing wakes it
again on its own. The existing design proposes auto-creating a
follow-up on every non-bot comment, but that means casual comments
(thinking out loud, ack-ing, mid-design notes) all spend tokens, and
forces the user to think about which comments will or won't trigger
the agent.

We want the cloud to **periodically re-invoke** the agent on
in-progress issues. Comments are **inert by default**. The user has
one explicit affordance — a **Comment & Run** button — to fire an
out-of-band run.

## 2. Goal

For an issue in In Progress, hand the agent the full context (issue
body + comment thread + last run's done payload) on a fixed cadence
and let it self-assess what to do this turn:

- continue working,
- exit cleanly with `noop` ("nothing has changed yet"),
- ask a question with `paused`,
- mark `completed` / `blocked`.

Defaults:

- **3-hour cadence** per issue.
- **24-tick hard cap** before the issue auto-transitions to a new
  **Paused** state. At the default 3h cadence that's 72 hours (3
  days) of ticking before auto-pause.

Both settings configurable at project level (default for all issues)
and overridable per issue.

Non-goals:

- Detecting "real progress" beyond the agent's own done-signal status.
  No file-diff or PR-event introspection. The cap is a simple total
  count of tick fires.
- Replacing PAUSED / pinning / native session resume. Periodic ticks
  dispatch through the same continuation code path that exists today;
  only the trigger changes.

## 3. Orientation: two state machines

This doc references two distinct state machines that are easy to
conflate. Pin them down before reading further:

| Name                | Visibility                                            | Values                                                                                                                                          | Owner                                                                                                                                          |
| ------------------- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| **Issue state**     | User-visible on the board                             | `backlog`, `todo`, `in progress`, `done`, `cancelled`, plus the new **paused** added by this design                                             | Mostly the user; the system auto-transitions In Progress → Paused at cap-hit; the agent may _suggest_ transitions via its done-signal payload. |
| **AgentRun status** | Internal / operator (TUI, metrics, run-history panel) | `QUEUED`, `ASSIGNED`, `RUNNING`, `AWAITING_APPROVAL`, `AWAITING_REAUTH`, `BLOCKED`, `COMPLETED`, `FAILED`, `CANCELLED`, `PAUSED_AWAITING_INPUT` | Cloud + runner. Each agent invocation has its own row; many runs accumulate over an issue's life.                                              |

The two are **independent**. An AgentRun terminating as `COMPLETED`
does not change the issue's state on the board. An AgentRun going
`BLOCKED` does not change it either. Only the user, or the agent's
done-signal explicitly requesting a state transition (via
`state_transition.requested_group` in `done_signal.py:_normalize`),
moves the issue.

**Naming hazard.** We have two "paused" concepts:

- **Paused** (issue state, new in this design) — work is parked, ticking
  is disarmed, user-visible. Set at cap-hit or manually.
- **PAUSED_AWAITING_INPUT** (AgentRun status, already exists per
  `.ai_design/issue_run_improve/design.md`) — a single run yielded
  with a question. Internal. Multiple can occur over an issue's life
  without ever auto-moving the issue to the new Paused state.

When this doc says "auto-transition to Paused" it means the **issue
state**. When it says `PAUSED_AWAITING_INPUT` it means the **run
status**.

## 4. Lifecycle

### 4.1 New issue state: Paused

Issue state model gains one new state, **Paused**, in the **Backlog**
group (work isn't actively happening; not Completed or Cancelled).

Both system and human can move an issue to Paused:

| Move                            | Trigger                   | Effect                                                                                                                        |
| ------------------------------- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| System auto-pause               | `tick_count >= max_ticks` | Disarm schedule, transition issue In Progress → Paused, surface red "not ticking" UI + workpad notice.                        |
| User manually moves to Paused   | UI action                 | Disarm schedule (the existing rule "schedule fires only in Started" handles this — moving out of Started kills the schedule). |
| User moves Paused → In Progress | UI action                 | Re-arm schedule fresh (`tick_count = 0`, immediate dispatch + first tick scheduled).                                          |

### 4.2 Arming the schedule

When an issue enters Started/In Progress:

1. Immediate dispatch fires (today's behavior preserved — the user
   clicks In Progress, the agent starts immediately).
2. Create or reset an `IssueAgentSchedule` row:
   - `interval_seconds` = issue override if set, else project default
     (3h).
   - `max_ticks` = issue override if set, else project default (24).
     `-1` = infinite.
   - `next_run_at` = `started_at + interval + jitter` (see §6.2).
   - `tick_count = 0`, `enabled = true`.

### 4.3 Tick fires

Each minute, the scanner finds rows where `enabled AND next_run_at <=
NOW() AND tick_count < max_ticks`, and for each one with no
`is_active` run on the issue:

1. Create the AgentRun via the existing continuation entry point
   (inherits prompt composition, runner pinning, drain).
2. On successful create: `tick_count += 1`, `last_tick_at = NOW()`,
   `next_run_at = NOW() + interval + jitter`.

If `is_active` is true (a run is in flight), the tick **skips** the
issue this minute. Neither `tick_count` nor `next_run_at` changes —
the scanner will re-check next minute, and the tick effectively waits
for the active run to finish. Skipped ticks do not consume cap budget
(the agent is still working, just slowly).

### 4.4 Disarming the schedule

| Trigger                                                               | Effect                                                                                                                                                                              |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Issue leaves Started (any state: Done, Cancelled, Backlog, Paused, …) | `enabled = false`.                                                                                                                                                                  |
| `tick_count >= max_ticks` after an increment                          | `enabled = false` immediately (no more fires). Auto-transition to Paused is **deferred**: it happens when the just-fired run terminates and no other active run exists. See §4.4.1. |
| Agent emits terminal `completed` / `blocked`                          | `enabled = false`. Issue state follows existing rules.                                                                                                                              |

#### 4.4.1 Deferred cap-hit pause

The cap-hit row above intentionally splits "stop firing" from "move
the issue to Paused." Setting `enabled = false` is immediate; the
state transition is deferred until the active run terminates.

Reason: the increment that pushes `tick_count` over the cap happens
right after a fresh `AgentRun` was created in §4.3, and that run is
still RUNNING. Moving the issue to Paused while a run is RUNNING
would either implicitly cancel the run or leave the issue in an
ill-defined "Paused with live run" state (cf. §10 Q6).

Implementation: a hook in `runner/consumers.py` terminate handlers
checks, after updating run state to a terminal status:

```
if schedule.enabled is False
   and issue.state.group == STARTED
   and no other is_active runs on issue:
       transition issue In Progress → Paused (system actor)
       post workpad notice; surface red "not ticking" UI
```

The hook is idempotent — if multiple terminate events fire
concurrently, only the first transition takes effect (DB constraint
on issue.state).

### 4.5 Re-arming

Triggered by issue entering Started. Idempotent reset — same logic
as §4.2 (creating/refreshing the schedule row, `tick_count = 0`,
`next_run_at = NOW() + interval + jitter`).

**Immediate dispatch is owned by the trigger, not by re-arming.**
The state-transition flow that brought the issue into Started is
responsible for firing the immediate run. Specifically:

- **State transition initiated by user (drag to In Progress, click a
  state button):** the state-transition handler fires the immediate
  dispatch (today's behavior).
- **State transition initiated by Comment & Run on a Paused issue
  (§4.6):** Comment & Run owns the dispatch. The state transition
  arms the schedule but does **not** fire its own dispatch.

This split avoids two dispatches per click and keeps the
single-active-run guardrail (`orchestration/service.py:_active_run_for`)
from silently no-op'ing one of them.

The existing pinning logic finds the latest prior run via
`parent_run`, so whichever path fires the dispatch resumes the
session naturally.

### 4.6 Comment & Run

The button:

1. Posts the user's comment.
2. Immediately fires a run (same continuation entry point as a tick).
3. Resets `tick_count = 0`.
4. Resets `next_run_at = NOW() + interval + jitter`.

Reset matches the spirit of "human just explicitly re-engaged — give
the agent a fresh budget."

**On a Paused issue:** Comment & Run is permitted, but before any
side effects the UI shows a confirmation dialog:

> "This issue is currently Paused. Running the agent will move it
> back to In Progress and resume periodic ticking."
>
> [Cancel] [Confirm]

On Cancel: nothing happens (comment is not posted either — the user
explicitly aborted the whole action). On Confirm, the flow is:

1. Post the comment.
2. Transition Paused → In Progress. This arms the schedule (per
   §4.5) but does **not** fire an immediate dispatch — Comment & Run
   owns the dispatch in this flow.
3. Comment & Run fires the run (single dispatch). The prompt builder
   includes the just-posted comment.
4. Apply the resets above (`tick_count = 0`, `next_run_at = NOW() +
interval + jitter`).

A regular **Comment** button (without Run) on a Paused issue just
posts the comment without any state change.

### 4.7 Plain Comment (default Enter)

Posts the comment. Inert with respect to the agent. The comment is
visible to the user, stored in `IssueComment`, and will be in the
agent's context the next time it runs (next tick, Comment & Run, or a
state transition).

## 5. Done-signal handling

The four agent done-signal statuses keep their existing terminal
semantics. Schedule effects:

| Status      | Run terminal status                     | Schedule effect                                                                                                                                                                              |
| ----------- | --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `completed` | `COMPLETED`                             | Disarm.                                                                                                                                                                                      |
| `blocked`   | `BLOCKED`                               | Disarm. User re-engages by moving issue back to In Progress.                                                                                                                                 |
| `noop`      | `COMPLETED` (today's mapping unchanged) | Schedule continues — next tick fires when due. The terminal `COMPLETED` doesn't block subsequent ticks because the schedule is independent of run status.                                    |
| `paused`    | `PAUSED_AWAITING_INPUT` (non-terminal)  | Schedule continues. Next tick will see no `is_active` run (PAUSED_AWAITING_INPUT is excluded from `is_active` per existing design) and fire normally, naturally continuing the conversation. |

No changes to `done_signal.ingest_into_run` are required. The trick
is that the schedule and the run are independent — the run's terminal
status doesn't gate further ticks.

The existing prompt-system fragments need a small update so the agent
understands periodic re-invocation and prefers a cheap `noop` over a
fabricated `paused` when there's literally nothing to ask.

## 6. Scanner

### 6.1 Mechanism

A Celery Beat task runs every minute. **Beat must run as a singleton
process** (one scheduler instance) — running multiple beat workers
duplicates the scan. This is standard Celery deployment hygiene.

```python
def scan_due_schedules():
    """Fan out fire_tick tasks. The actual claim happens in fire_tick."""
    due_ids = list(
        IssueAgentSchedule.objects
        .filter(enabled=True, next_run_at__lte=timezone.now())
        .filter(Q(max_ticks=-1) | Q(tick_count__lt=F('max_ticks')))
        .order_by('next_run_at')
        .values_list('id', flat=True)
    )
    for sched_id in due_ids:
        fire_tick.delay(sched_id)
```

`fire_tick` performs the **atomic claim** before dispatching. The
scanner read alone is not authoritative — multiple scans, an
overlapping Comment & Run, or a delayed task could otherwise
double-fire the same schedule:

```python
@shared_task
def fire_tick(sched_id):
    with transaction.atomic():
        sched = (
            IssueAgentSchedule.objects
            .select_for_update()
            .get(pk=sched_id)
        )
        # Re-check after locking. The world may have moved while we
        # waited for the row lock (Comment & Run reset us; another
        # tick fired; the schedule was disarmed).
        if not sched.enabled:
            return
        if sched.next_run_at is None or sched.next_run_at > timezone.now():
            return
        if sched.max_ticks != -1 and sched.tick_count >= sched.max_ticks:
            return
        if has_active_run(sched.issue):
            return  # tick is naturally deferred; will retry next minute

        # Claim: advance the schedule, then dispatch.
        sched.tick_count += 1
        sched.last_tick_at = timezone.now()
        sched.next_run_at = timezone.now() + sched.effective_interval() + jitter()
        sched.save(update_fields=[
            'tick_count', 'last_tick_at', 'next_run_at',
        ])

        # If this fire pushed us over the cap, mark the schedule
        # disabled now; the deferred-pause hook (§4.4.1) handles the
        # state transition when the just-created run terminates.
        if sched.max_ticks != -1 and sched.tick_count >= sched.max_ticks:
            sched.enabled = False
            sched.save(update_fields=['enabled'])

        # Now create the AgentRun via the existing continuation entry
        # point. _active_run_for is still the final guardrail.
        dispatch_continuation_run(sched.issue)
```

The same `select_for_update` + re-check pattern protects
Comment & Run when it touches the schedule (§4.6 resets
`tick_count` and `next_run_at`). Both code paths go through the same
locked region and serialize cleanly.

Per-issue clocks are independent. Issue A entering In Progress at
1:00 PM and Issue B at 1:30 PM produce schedules with `next_run_at`
4:00 PM and 4:30 PM. The scanner picks each up at its own time —
they don't collide.

### 6.2 Jitter

When `next_run_at` is set or reset (arming, tick advance,
Comment & Run), add a uniform random offset:

```
next_run_at = base + random(0, interval × 0.1)
```

For 3h cadence, that's a 0–18 minute spread. Without jitter, bulk
operations (sprint planning, 50 issues moved to In Progress in one
minute) would re-cluster every cycle.

### 6.3 Why scanner over per-issue scheduled tasks

- Schedule state lives in the DB — survives broker outages, single
  source of truth.
- Updates to interval / cap / disable are a DB write — no need to
  revoke and re-queue Celery tasks.
- Operational visibility: "what fires in the next hour?" is one SQL
  query.
- Sub-minute precision is unneeded at 3h cadence.

## 7. Schema

### 7.1 New model `IssueAgentSchedule`

```
issue              FK → Issue (1:1)

# User-configured overrides (null = inherit from project).
interval_seconds   integer, nullable     # null = inherit project default
max_ticks          integer, nullable     # null = inherit; -1 = infinite
user_disabled      boolean, default false  # user-set "never tick this issue"

# Runtime state.
next_run_at        datetime, nullable
tick_count         integer, default 0
last_tick_at       datetime, nullable
enabled            boolean, default true   # runtime arm/disarm flag

created_at, updated_at
```

Constraint: `issue` is unique. There is exactly one
`IssueAgentSchedule` row per issue; arming, disarming, and changing
overrides mutate that row in place rather than creating additional
schedule rows.

**`user_disabled` vs `enabled`** are intentionally separate fields:

- `user_disabled` is **user intent**, persistent across state
  transitions. When the user toggles "disable ticking" on an issue,
  this flag is set; re-arming the schedule (§4.5) respects it and
  leaves `enabled = false`.
- `enabled` is **runtime state**: true while the schedule should
  fire ticks, false when disarmed (issue out of Started, cap hit,
  terminal done-signal, or `user_disabled` is true). Reset at every
  arm/disarm event.

The scanner predicate is `enabled = true` — it doesn't read
`user_disabled` directly, because `user_disabled = true` should
already imply `enabled = false`.

Index: `(enabled, next_run_at)` for the scanner query.

### 7.2 Project additions

```
agent_default_interval_seconds  integer, default 10800   # 3h
agent_default_max_ticks         integer, default 24      # -1 = infinite (3 days @ 3h cadence)
agent_ticking_enabled           boolean, default true
```

**`agent_ticking_enabled = false` suppresses periodic ticks only.**
The initial In Progress dispatch (state-transition trigger) and the
explicit Comment & Run button still work. Rationale: users who want
the agent available on demand but not on autopilot need a single
toggle; users who want the agent fully removed should remove the
agent integration from the project (different feature, out of
scope for this design).

When `agent_ticking_enabled = false` at the project level: arming
logic creates the `IssueAgentSchedule` row but sets `enabled = false`
(so the scanner skips it). Re-enabling the project setting flips
schedules back to `enabled = true` for issues currently in Started
that don't have `user_disabled = true`.

### 7.3 Issue state model

Add Paused to the workspace state template, in the Backlog group.

### 7.4 No changes to `AgentRun`

Optional observability field `triggered_by` (`state_transition` /
`tick` / `comment_and_run`) could be added for analytics but is **not**
load-bearing on the design — defer until a use case appears.

## 8. UI surfaces

### 8.1 Issue detail page

Comment composer has two buttons (PR #62 already added Run AI and
Comment & Run — verify naming and behavior align with this design):

- **Comment** (default, Enter key): post comment only. Inert.
- **Comment & Run**: post comment + immediate run + reset
  `next_run_at` and `tick_count`.

Status row near the composer:

- Normal: "Next agent check: in 2h 14m" (driven by `next_run_at`).
- Cap hit: red text "Agent has stopped polling — issue moved to
  Paused after 24 ticks (3 days). Click Comment & Run or move back
  to In Progress to resume."

### 8.2 Project create / edit page

New section "AI agent ticking":

- Enabled (boolean, default on).
- Default cadence (interval picker: 30m / 1h / 3h / 6h / 12h / 24h /
  custom — default 3h).
- Max ticks before pause (integer, default 24, with an "infinite"
  option). At the default 3h cadence, 24 ticks = 3 days.

### 8.3 Issue settings

Per-issue overrides, written to `IssueAgentSchedule`:

| UI field                       | DB field           | Semantics                                                                                                                |
| ------------------------------ | ------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| Cadence override               | `interval_seconds` | Empty = inherit project default; explicit value = override.                                                              |
| Max ticks override             | `max_ticks`        | Empty = inherit; explicit positive = override; "infinite" = `-1`.                                                        |
| Disable ticking for this issue | `user_disabled`    | Boolean. When true, schedule arms with `enabled = false`; ticks never fire for this issue regardless of project default. |

The "disable ticking" toggle is distinct from the project-level
`agent_ticking_enabled`: project-level disables ticks for the whole
project; per-issue `user_disabled` disables ticks for one issue
even when the project has ticking on.

## 9. Behavior changes from existing code

The following are **removed** under this design:

- `orchestration/signals.py` `post_save(IssueComment)` receiver — the
  source of automatic comment-triggered runs.
- `orchestration/service.py:maybe_continue_after_terminate` — the
  terminate-side comment sweep. The schedule replaces its function.
- The call sites in `runner/consumers.py` terminate handlers that
  invoke `maybe_continue_after_terminate`.

The following are **kept**:

- `orchestration/service.py:handle_issue_comment` — repurposed as the
  dispatch entry point that **Comment & Run** calls explicitly. The
  bot/state/coalesce gating logic stays valuable; it just stops being
  invoked automatically on every comment save.
- `_create_continuation_run`, runner pinning, `build_continuation`
  prompt composition, drain — all unchanged.
- `done_signal.ingest_into_run` — unchanged (see §5).
- `PAUSED_AWAITING_INPUT` status, `pinned_runner_id`, native session
  resume — all unchanged from `.ai_design/issue_run_improve/design.md`.

## 10. Open questions

| #   | Question                                                                                                                                                                                                                                                                                                                                          |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Q1  | Migration: when this ships, do we retroactively create `IssueAgentSchedule` rows for existing In Progress issues? Probably yes, with `next_run_at = NOW() + interval + jitter`.                                                                                                                                                                   |
| Q2  | Is the new "Paused" state added once to the global state template, or per-workspace as part of an opt-in? Simplest: ship in the standard template.                                                                                                                                                                                                |
| Q3  | Does the system-driven In Progress → Paused transition show in the issue activity feed, and as which actor (system / agent / pi_dash_agent bot)? Probably "system."                                                                                                                                                                               |
| Q4  | If the workspace has a custom state in the Started group named something other than "In Progress," does ticking apply? Today's `_is_delegation_trigger` only matches the literal "In Progress" name. We should decide whether ticking inherits that constraint.                                                                                   |
| Q5  | Quiet hours / business-hours awareness — do we suppress ticks outside working hours? Out of scope for v1 unless requested.                                                                                                                                                                                                                        |
| Q6  | If the user **manually** moves an issue to Paused while a run is `RUNNING`, what happens to the run? Resolved for v1: disarm future ticks immediately, let the current run finish, and do not auto-resume until the user explicitly re-enters In Progress. This matches the deferred cap-hit behavior and avoids implicit cancellation semantics. |
| Q7  | What does the UI show when `max_ticks = -1` (infinite)? Probably "no cap" plus the running tick count.                                                                                                                                                                                                                                            |

## 11. Summary of decisions

1. **Scanner-based periodic ticking.** Celery Beat runs every minute,
   queries `IssueAgentSchedule`, fans out `fire_tick` tasks per due row.
2. **Default 3h cadence, default 24-tick hard cap** (= 3 days at 3h
   cadence). Configurable at project + issue level. `-1` means
   infinite.
3. **Per-issue independent clocks** — issues entering In Progress at
   different times have staggered `next_run_at`. Plus jitter on every
   schedule write to keep bulk transitions from re-clustering.
4. **Comments are inert.** Plain Enter = comment only. Only the
   explicit **Comment & Run** button wakes the agent out of band.
5. **Existing comment auto-trigger and terminate-side sweep are
   removed.** Continuation dispatch logic is kept — Comment & Run
   reuses it.
6. **Lifecycle:** arm on Started entry, disarm on Started exit / cap
   hit / `completed`+`blocked`. Re-arm on re-entering Started.
7. **New state "Paused"** in the Backlog group. System auto-moves
   issues there on cap hit; users can manually park work there.
8. **Comment & Run resets both** `next_run_at` (so the user doesn't
   wait through the remaining cadence on top of their explicit run)
   **and `tick_count`** (fresh budget after explicit re-engagement).
9. **No changes to `done_signal` or `AgentRun`.** The trick is that
   schedule lifecycle is independent of run terminal status — `noop`
   → `COMPLETED` doesn't prevent the next tick because the scanner
   reads schedules, not runs.
10. **UI:** two buttons on the comment composer, project-level
    settings on create/edit, per-issue overrides in settings, red
    "not ticking" indicator when capped.
11. **Atomic claim in `fire_tick`** (§6.1): the scanner only fans out
    candidate IDs; the actual claim (re-check + tick_count
    advance) happens inside `fire_tick` under
    `select_for_update`. Beat must be a singleton.
12. **Cap-hit auto-pause is deferred** (§4.4.1): cap-hit immediately
    sets `enabled = false` (no more fires), but the In Progress →
    Paused state transition waits until the run-terminate hook
    sees no active runs. This prevents "Paused with live RUNNING
    run" states.
13. **`user_disabled` (per-issue) and `agent_ticking_enabled`
    (per-project)** are explicit user-intent fields, distinct from
    runtime `enabled`. Both suppress periodic ticks only — initial
    In Progress dispatch and Comment & Run still work.
14. **Comment & Run on a Paused issue**: Comment & Run owns the
    single dispatch; the Paused → In Progress state transition
    arms the schedule but does not fire its own dispatch (§4.5).
    Avoids two-dispatch race against `_active_run_for`.
15. **One schedule row per issue.** `IssueAgentSchedule.issue` is
    unique; override edits and arm/disarm events mutate the same row.

---

## Appendix A — Worked timeline

A concrete trace. Cadence 3h, cap 24, no jitter shown for clarity.

```
T=0     issue A → In Progress
        immediate dispatch: AgentRun A1 created, fired
        IssueAgentSchedule(A): tick_count=0, next_run_at=T+3h
T+15m   user adds plain comment. Schedule untouched. Agent unaware.
T+30m   A1 emits paused (question), goes PAUSED_AWAITING_INPUT
        is_active(A) = false (PAUSED is not active)
        Schedule unchanged.
T+1h    user adds another plain comment. Still inert.
T+3h    scanner: A is due, no is_active run.
        AgentRun A2 created (continuation, parent=A1, pinned to A1's
        runner, native session resume).
        Schedule(A): tick_count=1, next_run_at=T+6h
        A2 reads issue + comments + A1's done_payload, decides
        nothing actionable yet, emits noop → COMPLETED.
T+6h    scanner fires A3. tick_count=2. Same as above.
...
T+72h   scanner fires A24. tick_count=24.
        After this fire: tick_count == max_ticks → schedule
        enabled=false immediately. Issue stays In Progress;
        A24 is RUNNING.
T+72h+ε A24 terminates (any status). Run-terminate hook (§4.4.1)
        sees enabled=false, issue still STARTED, no active runs.
        Auto-transitions issue In Progress → Paused (system actor).
        Workpad notice posted; UI shows red "not ticking."
T+96h   user looks at the issue, types an answer, clicks Comment & Run.
        UI shows confirmation: "Issue is Paused — running will move
        it back to In Progress." User clicks Confirm.
        Comment & Run flow (§4.6 on Paused):
          1. Comment posted.
          2. Issue transitions Paused → In Progress; schedule armed
             with tick_count=0, next_run_at=now+3h+jitter, BUT
             immediate dispatch is skipped (§4.5 — Comment & Run
             owns dispatch).
          3. Comment & Run fires AgentRun A25 (single dispatch),
             parent=A24, pinned to A24's runner if still online.
```
