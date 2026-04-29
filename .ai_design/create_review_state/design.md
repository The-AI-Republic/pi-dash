# In Review State — Code Review Ticking

> Directory: `.ai_design/create_review_state/`
>
> **Status:** revised after design review (see `.ai_design/create_review_state/`
> review notes in conversation history). Six findings applied: fresh
> session on cross-group entry, explicit terminal-signal disarm,
> review-specific cadence in v1, drop `last_ticking_state`, expand
> `CONTINUATION_ELIGIBLE_GROUPS`, merge the lifecycle and prompt PRs.
> This revision also resolves the follow-on implementation-readiness
> gaps: `noop` vs true completion, terminal disarm vs cap-hit
> auto-pause, preserving per-issue overrides across phase changes,
> explicitly restoring the pre-review implementation session, and
> enumerating the non-orchestration state-group call sites that must
> learn about `review`.
>
> **Scope:** add a new state group **Review** with a default state
> **In Review** that periodically wakes the AI agent on the same
> tick infrastructure as **In Progress**, but with a different
> prompt that drives **code review** rather than implementation.
>
> **What this changes about today's code**
>
> The Issue Ticking System (`.ai_design/issue_ticking_system/`) shipped a
> per-issue `IssueAgentTicker`, a Celery Beat scanner, atomic claim, and
> deferred cap-hit pause. This design generalizes the hard-coded
> `"In Progress"` gate into a `(group, state-name) → prompt-key`
> registry, adds review-specific cadence at the registry layer, forces
> a fresh agent session on cross-group transitions, and adds a missing
> "disarm on terminal `completed`/`blocked` signal" hook that the
> existing design specified but the existing code does not implement
> (verified — `disarm_ticker` is only called from the state-transition
> handler today; the runner-consumer terminate paths invoke only
> `maybe_apply_deferred_pause`, which itself requires a *pre-disarmed*
> ticker to act).

## 1. Problem

Today the agent only ticks for one state. There's no way to give the
agent a *different* responsibility (e.g., code review) on a periodic
cadence, because:

1. `bgtasks/agent_ticker.py:112` (`fire_tick`) gates on the literal
   string `"In Progress"`.
2. `orchestration/service.py:_is_delegation_trigger` (lines 77–85)
   gates the arming-on-state-entry path on the same literal string in
   the `STARTED` group.
3. `orchestration/service.py:206` `CONTINUATION_ELIGIBLE_GROUPS` is
   `(StateGroup.STARTED.value,)` — comments on issues outside Started
   never wake the agent.
4. `prompting/composer.py:60-65` always loads the workspace template
   named `coding-task` (`PromptTemplate.DEFAULT_NAME`) regardless of
   what state the issue is in.

So even if a workspace adds a custom Review-ish state today, the
ticker won't fire on it, comments on it won't wake the agent, and
even if either did, the agent would receive the implementation prompt.

## 2. Goal

Add a code-review phase that the agent can drive autonomously on a
ticking cadence, distinct from the implementation phase:

- A new **Review** state group, parallel to `STARTED`.
- A default **In Review** state in that group, seeded into every
  project (existing + new), color and sequence chosen to sit
  naturally between In Progress and Done on the board.
- Periodic ticking on **In Review** uses the same ticker primitives
  as **In Progress** (same row, same scanner, same cap path, same
  Comment & Run reset semantics) but with **review-specific cadence
  defaults** (30 min × 6 ticks = 3 hours total review window) baked
  into a phase registry, distinct from the 3 h × 24 ticks =
  3 days of implementation cadence.
- The review run uses a new **`code-review`** prompt template that
  asks the agent to review the implementation: read the diff,
  comment on the change, request fixes, or approve.
- The review run starts on a **fresh agent session** (no `parent_run`
  / `pinned_runner` carry-over) so the `code-review` template body
  lands as the actual system prompt rather than as a user message
  appended to a resumed implementation session.
- Terminal `completed` / `blocked` signals **disarm the ticker**
  immediately, closing the gap in the existing implementation-mode
  flow at the same time.

Non-goals (v1):

- Per-project overrides for review cadence/cap. Phase defaults are
  hardcoded at the registry layer in v1; per-project
  `agent_review_*` fields are a clean follow-up if review needs
  variable rhythm.
- Auto-transition In Progress → In Review on a particular agent
  done-signal. v1 uses manual user transitions only (matching how
  In Progress is entered today). A done-signal-driven hand-off is a
  follow-up.
- Generalization to "any state in any group ticks." v1 keeps the
  per-group designated-state-name model: the literal `"In Progress"`
  in `started`, the literal `"In Review"` in `review`. Custom
  workspace-named states in either group don't tick — same
  constraint as today, no regression.

## 3. Design — group-based phase registry

The existing ticker hard-codes one name in three places. v1 of this
design replaces all three with a single registry that maps state
groups to phase metadata:

```python
# pi_dash/orchestration/agent_phases.py  (new module — see §7.3)

@dataclass(frozen=True)
class PhaseConfig:
    state_name: str           # the literal state name that ticks in this group
    template_name: str        # PromptTemplate.name to render
    interval_seconds: int     # default cadence for this phase
    max_ticks: int            # default cap for this phase
    fresh_session_on_entry: bool  # if True, drop parent_run/pinned_runner
                                  # on entering this phase from a different
                                  # ticking phase
    disarm_on_completed: bool = True  # disarm ticker on terminal
                                       # completed/blocked
                                       # (kept on the registry for
                                       # explicitness; v1 sets True for
                                       # every entry)


PHASES: dict[str, PhaseConfig] = {
    StateGroup.STARTED.value: PhaseConfig(
        state_name="In Progress",
        template_name=PromptTemplate.DEFAULT_NAME,  # "coding-task"
        interval_seconds=10800,                      # 3 h
        max_ticks=24,                                # 3 days
        fresh_session_on_entry=False,
    ),
    StateGroup.REVIEW.value: PhaseConfig(
        state_name="In Review",
        template_name="code-review",
        interval_seconds=1800,                       # 30 min
        max_ticks=6,                                 # 3 h total
        fresh_session_on_entry=True,
    ),
}


def is_ticking_state(state) -> bool:
    if state is None:
        return False
    cfg = PHASES.get(state.group)
    return cfg is not None and state.name == cfg.state_name


def phase_config_for(state) -> Optional[PhaseConfig]:
    if state is None:
        return None
    cfg = PHASES.get(state.group)
    if cfg is None or state.name != cfg.state_name:
        return None
    return cfg


def template_name_for(state) -> str:
    cfg = phase_config_for(state)
    return cfg.template_name if cfg else PromptTemplate.DEFAULT_NAME
```

The ticker, the state-transition trigger, and the prompt composer
each consult this registry instead of hard-coded strings. Adding a
future phase (e.g., `qa`) is a single registry entry plus a
prompt-template seed.

### 3.1 Why one registry instead of three

We considered separating "what arms a ticker," "what fires a tick,"
and "what prompt to use." In practice the three travel together:
every state that ticks is also a state that arms, and a ticking
state needs *some* prompt to render. Threading a single registry
through all three call sites is simpler than three parallel
registries kept in sync.

### 3.2 Why phase defaults are not project fields in v1

Adding `agent_review_default_interval_seconds`,
`agent_review_default_max_ticks`, etc. to `Project` doubles the
project schema for a feature whose right defaults are
phase-intrinsic, not project-intrinsic (every project's review
cadence wants to be short for the same reason: diffs go stale).
v1 hardcodes the phase defaults; if a workspace later wants a
different review rhythm, we add the project fields then.

## 4. Lifecycle

### 4.1 New state group: Review

`StateGroup` enum (`db/models/state.py:14-20`) gains:

```python
REVIEW = "review", "Review"
```

A `State` row named **In Review** is added to `DEFAULT_STATES` and to
`seeds/data/states.json`, sequence between `In Progress` (35000) and
`Done` (45000) — pick **40000**. Color suggestion: distinct from In
Progress amber and Done green; e.g., indigo `#5B5BD6` (final color
during impl).

`DEFAULT_STATES` is the dominant seed path
(`api/views/project.py:240-254`, `app/views/project/base.py:288`
seed every project at creation); `seeds/data/states.json` is read
only by the first-workspace seed task
(`bgtasks/workspace_seed_task.py:189`). Both must be updated; the
in-Python default is the load-bearing one.

### 4.2 Lifecycle parity with In Progress

Treat **In Review** as a sibling of In Progress with respect to the
ticker lifecycle. All four touchpoints already implemented in
`orchestration/scheduling.py` and `bgtasks/agent_ticker.py` work
unchanged once they consult the registry:

| Event                                            | Behavior                                                                                                                                                |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Issue enters In Review                           | `_is_delegation_trigger` returns True (registry hit) → `arm_ticker` (writing review-phase defaults to the row) + immediate dispatch on a fresh session. |
| Periodic tick on In Review                       | `fire_tick` registry-checks, claims, calls `dispatch_continuation_run` — same atomic-claim flow as In Progress.                                         |
| Issue leaves the Review group                    | `disarm_ticker`. Generalized rule: "leaving a ticking group disarms" (was "leaving Started disarms").                                                   |
| Cap hit during In Review                         | Disarm immediately + deferred In Review → Paused on run terminate. Same `maybe_apply_deferred_pause` hook, gate generalized to ticking groups.          |
| Agent emits `completed` or `blocked`             | Ticker disarms via the new terminal-signal hook (§4.5). Issue stays In Review until the human transitions.                                              |
| Comment on In Review (incl. Comment & Run)       | Triggers continuation. Requires `CONTINUATION_ELIGIBLE_GROUPS` to include `REVIEW` (§7.4).                                                              |
| Comment & Run on Paused issue formerly in Review | Re-opens to In Progress (v1 default). Resuming directly to In Review is a follow-up that needs a small UI dialog choice.                                |

### 4.3 Cross-group transition: fresh session

User moves an issue **In Progress → In Review**:

1. From-group is `started`, to-group is `review`.
2. The "leaving a ticking group disarms" rule (generalized from
   `service.py:118-123`) fires `disarm_ticker(issue)`. The disarm is
   transient — it'll be re-armed in step 3 — but it's correct
   bookkeeping.
3. `_is_delegation_trigger(in_review_state)` returns True via the
   registry. `arm_ticker` reads `phase_config_for(state)` and writes
   review-phase defaults (30 min interval, 6-tick cap) to the
   ticker row. `tick_count = 0`,
   `next_run_at = NOW() + interval + jitter`, `enabled` re-evaluated.
4. Because `phase_config_for(state).fresh_session_on_entry` is True
   for the Review phase, the state-transition handler dispatches the
   first In Review run with `parent_run=None` and clears
   `pinned_runner_id`. The run starts a fresh agent session; the
   `code-review` template body becomes the system prompt, not a
   continuation message on top of the implementation conversation.

The fresh-session decision is the doc's most consequential choice.
The alternative — resuming the implementation session and rendering
the `code-review` template as a user-turn message — leaves the
implementation system prompt baked into the agent's session memory.
The agent ends up reading "you are implementing this task" plus a
user message saying "actually switch modes and review." Mid-
conversation re-roling is unreliable. Starting fresh costs the
implementation context (the agent must re-read the issue / diff)
but makes the phase signal unambiguous and lets the prompt template
do its job.

In Review → In Progress (user kicks the issue back for more work):
mirror image, with `fresh_session_on_entry=False` for the Started
phase. The state-transition handler does **not** use the most recent
prior run (which is now a review run); it restores the explicit
`ticker.resume_parent_run` captured on entry to Review so the agent
resumes the implementation session it left.

### 4.4 Done-signal handling

The four agent done-signal statuses keep their existing terminal
semantics regardless of phase. New in this design: a generalized
terminal-signal hook that disarms the ticker on true `completed` /
`blocked` for any phase. The hook is added in `scheduling.py` and
called from the runner consumer's terminate paths alongside
`maybe_apply_deferred_pause`:

```python
def maybe_disarm_on_terminal_signal(run: AgentRun) -> bool:
    """Disarm the ticker if the run's done-payload status is a
    phase-final signal (`completed` or `blocked`). Do not disarm on
    `noop`, even though `noop` persists today as
    AgentRunStatus.COMPLETED. Closes the gap in the existing
    issue-ticking design (which specifies disarm-on-terminal but
    whose code only invokes disarm_ticker from the state-transition
    handler). Returns True when a disarm was applied.
    """
```

| Status      | Run terminal status                     | Ticker effect                                                                                                                                                                   |
| ----------- | --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `completed` | `COMPLETED`                             | **Disarm** (new hook). Issue state unchanged — human transitions to Done (or, in review mode, accepts the approval and moves the issue forward).                               |
| `blocked`   | `BLOCKED`                               | **Disarm** (new hook). Human re-engages by moving the issue or commenting.                                                                                                      |
| `noop`      | `COMPLETED` (today's mapping unchanged) | Ticker continues — next tick fires when due. The hook must inspect `run.done_payload["status"]`, not just `run.status`, because `noop` and true completion currently share the same persisted run status. |
| `paused`    | `PAUSED_AWAITING_INPUT` (non-terminal)  | Ticker continues. Next tick will see no `is_active` run and fire normally.                                                                                                      |

The `noop`/`paused` exception to disarm is what makes ticking
useful: the agent can self-park while keeping the ticker alive
for the next human nudge.

The code-review prompt should make clear what each signal means in
review context:

- `completed` = "approved, ready to merge"
- `blocked` = "review found a blocker, work needed"
- `paused` = "I have a question for the human reviewer"
- `noop` = "diff hasn't changed since my last review"

### 4.5 Auto-pause on cap

Identical to In Progress. After 6 ticks (review default),
`enabled = false` immediately and the issue auto-transitions In
Review → Paused on the next run terminate via
`maybe_apply_deferred_pause`. The hook's gate generalizes from
"issue is in STARTED group with In Progress name" to
`is_ticking_state(state)`, **but** auto-pause remains reserved for
cap-hit disarms only. Terminal-signal disarm (`completed` /
`blocked`) must leave the issue in place for the human to act.

To make that distinction explicit, the ticker row tracks a
`disarm_reason` runtime field:

```python
class TickerDisarmReason(models.TextChoices):
    NONE = "", "None"
    LEFT_TICKING_STATE = "left_ticking_state", "Left Ticking State"
    CAP_HIT = "cap_hit", "Cap Hit"
    TERMINAL_SIGNAL = "terminal_signal", "Terminal Signal"
    USER_DISABLED = "user_disabled", "User Disabled"
```

`disarm_ticker(issue, *, reason=...)` persists the cause.
`maybe_apply_deferred_pause` only transitions the issue when
`disarm_reason == CAP_HIT`. That preserves the current deferred-pause
semantics for cap exhaustion while preventing successful review or a
blocked review from being immediately moved to Paused.

The Paused state stays in the Backlog group; re-engagement (Comment &
Run on Paused, or manual move) returns the issue to In Progress (v1
default — see §11 Q6).

## 5. Prompt template: `code-review`

A new global `PromptTemplate` row, `name="code-review"`,
`workspace=NULL`, `is_active=True`. Body sketch (final wording is
prompt-engineering work, not design):

```
You are reviewing an implementation, not implementing it.

Issue: {{ issue.name }}
Description: {{ issue.description_stripped }}
Recent activity:
{{ comments_section }}
Latest implementation run output:
{{ parent_done_payload }}

Your job this turn:
- Read the diff (use git tooling) against the project's main branch.
- Identify correctness, security, and design issues.
- If the change looks good, emit `completed` with a short approval note.
- If you find issues, comment on them and emit `blocked`.
- If you have a clarifying question, emit `paused` with the question.
- If nothing has changed since your last review, emit `noop`.

Avoid making code changes yourself unless explicitly asked. Default
to leaving review comments.
```

Seed and migration:

- Add `CODE_REVIEW_TEMPLATE_BODY` constant alongside the existing
  default in `prompting/seed.py` (or wherever the global default is
  seeded today — verify during impl).
- Data migration: insert the row if it doesn't exist; idempotent.
- Add a reseed command analogous to
  `prompting/management/commands/reseed_default_template.py` for
  operator use during prompt iteration.

### 5.1 Why the template only needs to render on the first run

`build_continuation` (`composer.py:67-97`) returns the concatenation
of new human comments since the parent run started, with no template
involvement. That's the path 99% of ticks take. Phase intent does
**not** need to be re-stated each tick — the agent's session memory
already carries the `code-review` system prompt that landed on the
fresh-session entry (§4.3). Continuation messages are intentionally
minimal: "here is what humans said since you last ran."

The only template-rendered run in In Review is the *first* one (on
state entry). Every subsequent tick is comment-delta only. This is
why the §4.3 fresh-session decision is load-bearing — without it,
the very one render that establishes phase intent is rendered as a
user-message on a resumed implementation session, where it doesn't
re-role the agent.

The fallback inside `build_continuation` (when no
`parent.started_at` exists) does need to consult the registry to
pick the right template name — see §7.7.

## 6. Schema

### 6.1 New StateGroup enum value

`db/models/state.py:StateGroup` gets `REVIEW = "review", "Review"`.

This is an **enum value addition** to a `models.TextChoices` class.
The DB stores group as `CharField(choices=...)`, so the migration
is just a `state.choices` update. No data migration is required for
the enum itself; existing rows are unaffected.

### 6.2 Seed `In Review` state per project

Two pieces, mirroring what M2 in the issue-ticking-system design did
for `Paused`:

1. Update `seeds/data/states.json` to include an `In Review` entry
   in the `review` group, sequence 40000, default color picked.
2. Update `DEFAULT_STATES` in `db/models/state.py` (the dominant
   path — every API project creation uses this).
3. Data migration (`RunPython`): for every existing project that
   lacks an `In Review` state, create one. Idempotent.

### 6.3 New runtime fields on `IssueAgentTicker`

The earlier draft proposed no new ticker fields after dropping
`last_ticking_state`. That is not enough once the design is made
implementation-ready. Three runtime concerns need first-class
storage:

1. **Disarm cause** so deferred auto-pause can distinguish cap-hit
   disarms from terminal-signal disarms.
2. **Phase defaults** so review cadence can differ from
   implementation cadence without overwriting the user's existing
   per-issue override fields (`interval_seconds`, `max_ticks`).
3. **Resume parent** so leaving Review can explicitly resume the
   pre-review implementation session rather than guessing from the
   latest prior run in a mixed implementation/review ancestry chain.

`IssueAgentTicker` therefore gains:

```python
phase_default_interval_seconds = models.IntegerField(null=True, blank=True)
phase_default_max_ticks = models.IntegerField(null=True, blank=True)
disarm_reason = models.CharField(
    max_length=32,
    blank=True,
    default="",
    choices=TickerDisarmReason.choices,
)
resume_parent_run = models.ForeignKey(
    "runner.AgentRun",
    null=True,
    blank=True,
    on_delete=models.SET_NULL,
    related_name="+",
)
```

Semantics:

- `interval_seconds` / `max_ticks` remain the **user-configured
  issue-level overrides**. They are never repurposed to hold phase
  defaults.
- `phase_default_interval_seconds` / `phase_default_max_ticks` are
  runtime state written by `arm_ticker` from the phase registry. They
  reset on every re-arm.
- `effective_interval_seconds()` becomes:
  issue override → phase default → project default.
- `effective_max_ticks()` becomes:
  issue override → phase default → project default.
- `resume_parent_run` is populated on `started -> review` with the
  latest implementation-phase run. On `review -> started`, that run is
  used as the parent for the fresh implementation run; after a
  successful hand-back it is cleared or replaced with the newest
  implementation run.

This is intentionally runtime-only state. It is not user-facing and
does not change the meaning of the existing override fields.

### 6.4 No new project fields in v1

`agent_default_interval_seconds`, `agent_default_max_ticks`, and
`agent_ticking_enabled` are reused. Review-phase defaults live in
the `agent_phases.PHASES` registry, not on `Project`.

## 7. Code touchpoints

Eight files. Concentrated edits; the registry carries most of the
new policy.

### 7.1 `db/models/state.py` (enum + DEFAULT_STATES)

Add `REVIEW = "review", "Review"` to `StateGroup`. Add the In Review
entry to `DEFAULT_STATES`.

### 7.2 `orchestration/agent_phases.py` (new module)

Owns `PhaseConfig`, `PHASES`, `is_ticking_state(state)`,
`phase_config_for(state)`, `template_name_for(state)`. Importable
from both `orchestration/` and `prompting/` without introducing a
cycle (state model + prompting model are leaf imports).

### 7.3 `orchestration/scheduling.py`

- `arm_ticker`: read `phase_config_for(issue.state)` and write
  `phase_default_interval_seconds`,
  `phase_default_max_ticks`, clear `disarm_reason`, and reset
  `next_run_at`/`tick_count`. Do **not** overwrite
  `interval_seconds` / `max_ticks`; those remain the user's explicit
  per-issue overrides.
- `dispatch_continuation_run` (or its callers in
  `_create_continuation_run`): when
  `phase_config_for(state).fresh_session_on_entry` is True **and**
  the latest prior run is in a different phase, dispatch with
  `parent_run=None` and clear `pinned_runner_id`. v1 implements this
  by detecting the cross-phase entry inside the state-transition
  handler (which already has both `from_state` and `to_state`)
  rather than inferring it inside the dispatch helper.
- New `maybe_disarm_on_terminal_signal(run)`: inspect
  `run.done_payload["status"]`; disarm only on `completed` and
  `blocked`, never on `noop`. Persist
  `disarm_reason=TERMINAL_SIGNAL`. Idempotent. Safe to call
  alongside `maybe_apply_deferred_pause`.
- `maybe_apply_deferred_pause`: replace the
  `state.group == STARTED.value && state.name == "In Progress"` gate
  (lines 320-323) with `is_ticking_state(state)`, and require
  `disarm_reason == CAP_HIT` before transitioning to Paused.

### 7.4 `orchestration/service.py`

- `_is_delegation_trigger`: replace the body with
  `return is_ticking_state(to_state)`.
- `handle_issue_state_transition` disarm rule (lines 118-123):
  generalize from `STARTED → non-STARTED` to
  `is_ticking_state(from_state) AND NOT is_ticking_state(to_state)`.
  Inter-ticking-group transitions (In Progress → In Review) re-arm
  via the trigger check; the disarm-then-arm sequence is
  intentionally explicit even though the second call resets the
  same row.
- `handle_issue_state_transition` cross-phase fresh-session: when
  both `from_state` and `to_state` are ticking states in *different*
  groups, the immediate dispatch must clear `parent_run` /
  `pinned_runner_id` (per §4.3). Threading a `fresh_session: bool`
  flag through `_create_and_dispatch_run` is still the minimal
  creation-path change, but the resume source for the reverse
  transition must be explicit:
  - on `started -> review`, capture the latest implementation run
    into `ticker.resume_parent_run` before dispatching the fresh
    review session
  - on `review -> started`, prefer `ticker.resume_parent_run` as the
    new run's `parent_run` instead of `_latest_prior_run(issue)`
    (which now points at a review run)
  - derive the pinned runner from that same explicit parent
- `CONTINUATION_ELIGIBLE_GROUPS` (line 206): expand from
  `(StateGroup.STARTED.value,)` to
  `tuple(PHASES.keys())` — i.e., every group in the phase
  registry. Without this, comments on In Review issues never wake
  the agent.

### 7.5 `bgtasks/agent_ticker.py`

`fire_tick`: replace `issue.state.name != DELEGATION_STATE_NAME`
(line 112) with `not is_ticking_state(issue.state)`.

### 7.6 `runner/consumers.py`

Both terminate paths (`_handle_run_paused` ~line 636 and
`_finalize_run` ~line 708) currently call only
`maybe_apply_deferred_pause`. Add a sibling call to
`maybe_disarm_on_terminal_signal` — same `transaction.on_commit`
pattern, same exception-swallowing wrapper. Order matters: disarm
must run **before** `maybe_apply_deferred_pause` so the
deferred-pause hook sees the latest disarm reason. Because
`maybe_apply_deferred_pause` now requires `disarm_reason == CAP_HIT`,
the same ordering is safe for terminal-signal completes: the issue is
left in-place, not auto-paused.

### 7.7 `prompting/composer.py`

`build_first_turn`: pass `template_name_for(issue.state)` to
`load_template`. Same change inside `build_continuation`'s fallback
path. The non-fallback continuation path (just-the-new-comments)
needs no change — phase intent lives in the agent's session memory
established at the fresh-session entry.

### 7.8 `prompting/seed.py` + new template row

Add `CODE_REVIEW_TEMPLATE_BODY` constant. New data migration under
`prompting/migrations/` inserts the global row.

## 8. Migrations

Three migrations across the two PRs:

**PR A / M0 — terminal-disarm safety** under `db/migrations/`:

- `AddField` on `IssueAgentTicker.disarm_reason`.
- Backfill existing rows with the empty-string default.

**PR B / M1 — enum + In Review seed** under `db/migrations/`:

- `AlterField` on `State.group` to refresh choices.
- `AddField` × 3 on `IssueAgentTicker`:
  `phase_default_interval_seconds`,
  `phase_default_max_ticks`,
  `resume_parent_run`.
- `RunPython` data migration: for every project lacking an In Review
  state, create one in the `review` group, sequence 40000.
- `seeds/data/states.json` and `DEFAULT_STATES` updated alongside.

**PR B / M2 — code-review prompt template** under `prompting/migrations/`:

- `RunPython`: insert global
  `PromptTemplate(name="code-review", workspace=NULL, is_active=True,
  body=CODE_REVIEW_TEMPLATE_BODY)` if not present. Idempotent.

Both are reversible.

## 9. Tests

New tests:

- `orchestration/test_agent_phases.py` — registry behavior:
  `is_ticking_state`, `phase_config_for`, `template_name_for`,
  exhaustive over every StateGroup value.
- Extend `test_scheduling.py`:
  - `arm_ticker` writes phase-default interval/max for In Review
    without clobbering pre-existing issue overrides.
  - `disarm_ticker` triggered by leaving the Review group.
  - `maybe_apply_deferred_pause` works on In Review.
  - `maybe_disarm_on_terminal_signal` disarms on COMPLETED and
    BLOCKED payloads, no-ops on `noop` / `paused`, and persists the
    correct `disarm_reason`.
  - `maybe_disarm_on_terminal_signal` running before
    `maybe_apply_deferred_pause` produces auto-Pause on
    cap-hit terminate, but **not** on terminal-signal terminate.
- Extend `test_agent_ticker.py` (bgtasks): `fire_tick` ticks an In
  Review issue; skips an issue in a non-ticking state.
- Extend `test_service.py`:
  - `_is_delegation_trigger` returns True for In Review.
  - Cross-group transitions arm-then-rearm cleanly.
  - In Progress → In Review dispatches with `parent_run=None`
    (fresh session) **and** stores `ticker.resume_parent_run`.
  - In Review → In Progress dispatches with
    `ticker.resume_parent_run` as parent rather than the latest prior
    review run.
  - Comment on In Review wakes the agent (i.e.,
    `CONTINUATION_ELIGIBLE_GROUPS` includes REVIEW).
- New / extend `prompting/test_composer.py`:
  - `build_first_turn` selects `coding-task` for In Progress and
    `code-review` for In Review.
  - `build_continuation` fallback respects the same registry.
- Migration tests:
  - M1 idempotent across re-runs; only In Review rows added.
  - new ticker fields backfill with safe defaults
    (`NULL`/`""`) and do not mutate existing override values.
  - M2 idempotent; existing global `coding-task` row untouched.

## 10. UI

Minimal v1 surface — defer comprehensive UI:

- Project state-management view shows the new Review group as a
  column, **but this is not free**. The following existing hard-coded
  state-group enumerations must be updated in PR B:
  - `packages/constants/src/state.ts`
  - `apps/api/pi_dash/space/utils/grouper.py`
  - `apps/api/pi_dash/api/views/issue.py`
  - `apps/api/pi_dash/utils/order_queryset.py`
  - any generated/shared `TStateGroups` type source consumed by the
    frontend packages
- Issue detail page's "next agent check" status row already reads
  `next_run_at` from the ticker — no change.
- Cap-hit copy on a paused-from-review issue: same template as
  cap-hit-from-implementation in v1 (the phase-aware copy needs
  `last_ticking_state`, deferred per §6.3).

## 11. Open questions

| #   | Question                                                                                              | Decision                                                                                                                                                                                          |
| --- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Q1  | Native session resume across phase change?                                                            | **No — fresh session on cross-group entry.** §4.3. Code-review system prompt needs to actually be the system prompt, not a user message on a resumed implementation session.                      |
| Q2  | Separate cadence/cap settings for review?                                                             | **Yes, hardcoded at the registry layer.** §3, §3.2. Review = 30 min × 6 ticks = 3 h. Per-project overrides are a follow-up.                                                                       |
| Q3  | Auto hand-off In Progress → In Review via agent done-signal?                                          | **No (v1)** — manual user transition only. Needs `state_transition.requested_group` ingestion (parsed today, never consumed); out of scope.                                                       |
| Q4  | Custom workspace state names within the Review group (e.g., "QA")?                                    | **Don't tick (v1)** — same restriction as In Progress today. Generalize to "any state in a ticking group" in a separate change.                                                                   |
| Q5  | Should the code-review agent be allowed to write code, or strictly leave comments?                    | **Strictly leave comments (v1)** — the prompt says so. Loosen later if the boundary is too restrictive.                                                                                           |
| Q6  | What does Comment & Run on a Paused-from-Review issue resume to?                                      | **In Progress (v1)** — unconditional. A phase-aware re-open dialog is the proper fix; ships with the deferred `last_ticking_state` field.                                                         |
| Q7  | Same `pi_dash_agent` bot for review-mode runs, or a separate `pi_dash_reviewer` bot for activity feed UX? | **Same bot (v1).** Splitting is a one-line change later.                                                                                                                                          |
| Q8  | Does the new terminal-signal disarm hook also need to fire on In Progress runs (existing gap)?         | **Yes.** §4.4 / §7.6. The hook is phase-agnostic; it disarms on true `completed`/`blocked` regardless of which group the issue is in, but it explicitly skips `noop`. |

## 12. PR sequence

Two PRs, each independently shippable.

**PR A — Phase registry refactor + terminal disarm fix**

Replaces hard-coded constants with the registry. Registry is seeded
with only the `started` phase using current defaults; no new states,
no new templates. This PR also adds the runtime distinction needed to
make terminal disarm safe: `disarm_reason`.

- New `orchestration/agent_phases.py` with `PhaseConfig`, `PHASES`,
  `is_ticking_state`, `phase_config_for`, `template_name_for`.
- Replace literal `"In Progress"` checks in `service.py`,
  `bgtasks/agent_ticker.py`, `scheduling.py`, `composer.py` with
  registry calls.
- Add `IssueAgentTicker.disarm_reason` and gate
  `maybe_apply_deferred_pause` on `CAP_HIT` so a terminal disarm does
  not auto-pause the issue.
- Add `maybe_disarm_on_terminal_signal` hook in `scheduling.py` and
  wire it into both runner-consumer terminate paths. **This closes
  the existing terminal-signal disarm gap; ship with regression
  tests verifying In Progress runs that emit true `completed` or
  `blocked` correctly disarm the ticker, while `noop` does not.**
- Tests for the registry and the new hook.
- This PR is a refactor; ticker behavior for In Progress is
  identical to today *except* the terminal-signal disarm now
  actually fires (which is what the existing design specified), and
  terminal disarm no longer risks cascading into auto-Pause.

**PR B — Review state group + In Review state + code-review prompt**

Lights up code-review behavior end to end. PR B and the previously-
separate prompt PR are merged: shipping the state without the
prompt would render an implementation prompt against a
code-review-named state, which is worse than not shipping the state
at all.

- M1 migration (StateGroup enum + In Review seed + backfill).
- Update `DEFAULT_STATES` in `state.py`.
- M2 migration (insert `code-review` template).
- Add `CODE_REVIEW_TEMPLATE_BODY` to `prompting/seed.py`.
- Reseed command analogous to `reseed_default_template.py`.
- Add `REVIEW` entry to `agent_phases.PHASES` (with
  `fresh_session_on_entry=True`, review-phase cadence/cap).
- `arm_ticker` writes phase defaults to dedicated runtime fields on
  the row, not into the user override fields.
- Cross-phase fresh-session logic in `handle_issue_state_transition`
  / `_create_and_dispatch_run`, plus explicit
  `resume_parent_run` capture/use for the implementation hand-back.
- Expand `CONTINUATION_ELIGIBLE_GROUPS` to all keys in `PHASES`.
- Generalize `disarm_ticker` rule to "leaving a ticking group."
- Tests per §9.

A third PR may carry UI polish (phase-aware re-open dialog with
`last_ticking_state`).

---

## Appendix A — Worked timeline

```
T=0     Issue I in Todo
T+5m    User moves I → In Progress (group=started, name="In Progress")
        is_ticking_state ✓ → arm_ticker (phase defaults: 3h × 24),
        immediate dispatch with parent_run=None (no prior run).
        Run R1 created, prompt template "coding-task".
        Ticker: tick_count=0, next_run_at=T+3h+jitter.
T+1h    R1 emits completed → terminal.
        Consumer terminate: maybe_disarm_on_terminal_signal → ticker
        disarmed. (Old behavior: ticker kept ticking. New behavior:
        agent stays quiet until human acts.)
T+2h    User comments "looks good, but please move to review."
        Comment is inert — ticker is disarmed; comment alone doesn't
        wake the agent.
T+2h+1m User moves I → In Review.
        from_group=started, to_group=review:
          • disarm rule fires (transient).
          • is_ticking_state(In Review) ✓ → arm_ticker writes review
            phase defaults: interval=30min, max_ticks=6.
          • Cross-phase fresh-session detected → R2 dispatched with
            parent_run=None and pinned_runner_id cleared.
          • R2's first turn renders "code-review" template as the
            actual system prompt of a fresh agent session.
T+2h+30m+ε  R2 finishes its review, emits paused with a question.
        Consumer terminate: maybe_disarm_on_terminal_signal sees
        non-terminal status (paused) → no-op. Ticker still armed.
T+2h+1h Tick fires for In Review (30 min after the last next_run_at
        reset). R3 created, build_continuation returns concatenated
        new comments since R2's start (including the human's reply
        to R2's question, if any). Native session resume on R2's
        thread — code-review system prompt persists.
        Tick count = 2.
...
T+2h+3h Cap hit on the In Review ticker (6 ticks × 30 min = 3 h
        review window, plus the immediate-dispatch tick). enabled=
        false immediately. On the just-fired run's termination,
        maybe_apply_deferred_pause sees:
          • is_ticking_state(I.state) ✓ (In Review)
          • ticker disarmed
          • no other active runs
          → I → Paused (system actor).
T+later User clicks Comment & Run on the Paused issue.
        v1 dialog: "This issue is Paused. Running will move it back
        to In Progress." (Generic copy — phase-aware copy is a
        follow-up.) On Confirm: comment posted, I → In Progress,
        ticker re-armed with implementation defaults, R_{N+1}
        dispatched. The user can move the issue back to In Review
        manually if they want another review pass.
```
