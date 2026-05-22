# In Review State — Code Review Ticking

> Directory: `.ai_design/create_review_state/`
>
> **Status:** ready for implementation. Decisions resolved through
> review: fresh session on cross-group entry (§4.3), explicit
> terminal-signal disarm (§4.4), re-arm on human comment (§4.6),
> polymorphic `review` prompt with runtime mode inference — CODE /
> DESIGN / DESIGN*THEN_CODE / GENERIC (§4.7 / §5 / Q10), drop
> `last_ticking_state`, expand `CONTINUATION_ELIGIBLE_GROUPS`,
> merge the lifecycle and prompt PRs. **Cadence is split per phase
> on `Project`** (§3.2 / §6.4): existing `agent_default*_`narrows in semantic to In Progress; new`agent*review_default_interval_seconds`(3 h) and`agent_review_default_max_ticks` (8) drive review-phase
rhythm. Per-issue overrides split similarly
(`review_interval_seconds`/`review_max_ticks`). The
`effective*_()`resolver is phase-aware. Implementation-readiness
gaps closed:`noop`vs true completion, terminal disarm vs
cap-hit auto-pause, preserving per-issue overrides across phase
changes, explicitly restoring the pre-review implementation
session via`resume_parent_run`, and enumerating the
non-orchestration state-group call sites that must learn about
`review`.
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
> registry, adds phase-aware cadence on `Project` (review defaults
> distinct from impl, but configured in the same place), forces
> a fresh agent session on cross-group transitions, and adds a missing
> "disarm on terminal `completed`/`blocked` signal" hook that the
> existing design specified but the existing code does not implement
> (verified — `disarm_ticker` is only called from the state-transition
> handler today; the runner-consumer terminate paths invoke only
> `maybe_apply_deferred_pause`, which itself requires a _pre-disarmed_
> ticker to act).

## 1. Problem

Today the agent only ticks for one state. There's no way to give the
agent a _different_ responsibility (e.g., code review) on a periodic
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

Add a review phase that the agent can drive autonomously on a
ticking cadence, distinct from the implementation phase. "Review"
is polymorphic — the agent picks the right cycle (code review,
design review, design-then-code, or generic) from the work product
itself; see §4.7 for the inference rules.

- A new **Review** state group, parallel to `STARTED`.
- A default **In Review** state in that group, seeded into every
  project (existing + new), color and sequence chosen to sit
  naturally between In Progress and Done on the board.
- Periodic ticking on **In Review** uses the same ticker primitives
  as **In Progress** (same row, same scanner, same cap path, same
  Comment & Run reset semantics) but with **separate cadence
  config per phase** (see §3.2 / §6.4): review defaults to **3 h
  interval × 8 ticks (24 h review window)** vs implementation's
  3 h × 24 (3 days). The interval matches In Progress because the
  3 h gap is the human-involvement window (each tick is a complete
  review cycle, then the agent stops to make room for the human);
  the cap is shorter because review windows shouldn't sit open for
  multiple days.
- The review run uses a new **`review`** prompt template that
  routes between code review, design review, design-then-code, or
  generic review based on what the impl run produced (§4.7 / §5).
- The review run starts on a **fresh agent session** (no `parent_run`
  / `pinned_runner` carry-over) so the `review` template body
  lands as the actual system prompt rather than as a user message
  appended to a resumed implementation session.
- Terminal `completed` / `blocked` signals **disarm the ticker**
  immediately, closing the gap in the existing implementation-mode
  flow at the same time.

Non-goals (v1):

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
        fresh_session_on_entry=False,
    ),
    StateGroup.REVIEW.value: PhaseConfig(
        state_name="In Review",
        template_name="review",
        fresh_session_on_entry=True,
    ),
}

# Cadence (interval_seconds / max_ticks) is intentionally NOT on
# PhaseConfig — it stays centrally managed on `Project`, with
# per-issue overrides on `IssueAgentTicker`. v1 splits the cadence
# fields per phase so the In Progress and Review rhythms can
# differ. See §3.2 and §6.4.


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
state needs _some_ prompt to render. Threading a single registry
through all three call sites is simpler than three parallel
registries kept in sync.

### 3.2 Why cadence splits per phase but stays on `Project`

Cadence stays centrally managed (`Project` row + per-issue
override) — that's the existing pattern and we keep it. What v1
adds is a **phase axis**: each phase has its own pair of fields,
so review and implementation can have different rhythms without
either leaking into the code-level `PHASES` registry.

Defaults (rationale):

- **In Progress**: `agent_default_interval_seconds=10800` (3 h) ×
  `agent_default_max_ticks=24` (3-day budget). Unchanged from
  today.
- **In Review**: `agent_review_default_interval_seconds=10800`
  (3 h) × `agent_review_default_max_ticks=8` (24-h review window).
  Same interval as impl because each review tick is a complete
  cycle (find → validate → comment → apply → resolve → summary)
  and the 3 h gap exists to make room for human involvement
  between cycles. Shorter cap because a review that sits open for
  multiple days has gone stale.

`PhaseConfig` deliberately does not carry cadence — putting it
there would force a code deploy to retune review rhythm per
workspace, which defeats the central-management property. The
`PHASES` registry stays narrow: state name, template name,
fresh-session flag.

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

| Event                                            | Behavior                                                                                                                                                                                             |
| ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Issue enters In Review                           | `_is_delegation_trigger` returns True (registry hit) → `arm_ticker` (review-phase effective cadence, see §6.4) + immediate dispatch on a fresh session.                                              |
| Periodic tick on In Review                       | `fire_tick` registry-checks, claims, calls `dispatch_continuation_run` — same atomic-claim flow as In Progress.                                                                                      |
| Issue leaves the Review group                    | `disarm_ticker`. Generalized rule: "leaving a ticking group disarms" (was "leaving Started disarms").                                                                                                |
| Cap hit during In Review                         | Disarm immediately + deferred In Review → Paused on run terminate. Same `maybe_apply_deferred_pause` hook, gate generalized to ticking groups.                                                       |
| Agent emits `completed` or `blocked`             | Ticker disarms via the new terminal-signal hook (§4.5). Issue stays In Review until the human transitions.                                                                                           |
| Comment on In Review (incl. Comment & Run)       | Triggers continuation **and re-arms the ticker** (§4.6) so automatic ticking resumes if the previous run terminally disarmed it. Requires `CONTINUATION_ELIGIBLE_GROUPS` to include `REVIEW` (§7.4). |
| Comment & Run on Paused issue formerly in Review | Re-opens to In Progress (v1 default). Resuming directly to In Review is a follow-up that needs a small UI dialog choice.                                                                             |

### 4.3 Cross-group transition: fresh session

User moves an issue **In Progress → In Review**:

1. From-group is `started`, to-group is `review`.
2. The "leaving a ticking group disarms" rule (generalized from
   `service.py:118-123`) fires `disarm_ticker(issue)`. The disarm is
   transient — it'll be re-armed in step 3 — but it's correct
   bookkeeping.
3. `_is_delegation_trigger(in_review_state)` returns True via the
   registry. `arm_ticker` runs unchanged from today's path —
   `tick_count = 0`,
   `next_run_at = NOW() + effective_interval_seconds() + jitter`,
   `enabled` re-evaluated. The effective interval/max come from the
   issue override or the project default, identical to In Progress.
4. Because `phase_config_for(state).fresh_session_on_entry` is True
   for the Review phase, the state-transition handler dispatches the
   first In Review run with `parent_run=None` and clears
   `pinned_runner_id`. The run starts a fresh agent session; the
   `review` template body becomes the system prompt, not a
   continuation message on top of the implementation conversation.

The fresh-session decision is the doc's most consequential choice.
The alternative — resuming the implementation session and rendering
the `review` template as a user-turn message — leaves the
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

| Status      | Run terminal status                     | Ticker effect                                                                                                                                                                                                          |
| ----------- | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `completed` | `COMPLETED`                             | **Disarm** (new hook). Issue state unchanged. Either a human transition or any subsequent human comment will re-arm the ticker (§4.6); the disarm is "stop ticking until someone engages," not "stop ticking forever." |
| `blocked`   | `BLOCKED`                               | **Disarm** (new hook). Same re-arm-on-engagement semantics as `completed`.                                                                                                                                             |
| `noop`      | `COMPLETED` (today's mapping unchanged) | Ticker continues — next tick fires when due. The hook must inspect `run.done_payload["status"]`, not just `run.status`, because `noop` and true completion currently share the same persisted run status.              |
| `paused`    | `PAUSED_AWAITING_INPUT` (non-terminal)  | Ticker continues. Next tick will see no `is_active` run and fire normally.                                                                                                                                             |

The `noop`/`paused` exception to disarm is what makes ticking
useful: the agent can self-park while keeping the ticker alive
for the next human nudge.

The review prompt should make clear what each signal means
(slightly different shading per review kind — see §4.7 / §5):

- `completed` = "approved" (PR ready to merge / design accepted /
  generic work product looks good)
- `blocked` = "review found real issues that need human attention"
  (or that the agent could not auto-fix in this kind)
- `paused` = "I have a question for the human reviewer"
- `noop` = "the artifact hasn't changed since my last review pass"

### 4.5 Auto-pause on cap

Identical to In Progress. After the cap is hit (`effective_max_ticks()`
on the ticker — same value review and implementation see in v1),
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

### 4.6 Re-arm on human comment

Any human comment on an issue whose state group is in
`CONTINUATION_ELIGIBLE_GROUPS` re-arms the ticker, **even when the
last run terminally disarmed it** (`disarm_reason=TERMINAL_SIGNAL`).
This makes the ticking model match the user expectation:

> Comment is engagement. Engagement restarts automatic ticking.

Before this design, `handle_issue_comment` (`orchestration/service.py:209`)
only dispatched a one-shot continuation run; it never touched the
ticker row. Combined with the new terminal-signal disarm hook
(§4.4), that would leave the issue in a "comment fires once, then
silence" state — surprising to users who expect a comment to
restart automatic progress.

The fix:

```python
# Inside handle_issue_comment, after the bot/group eligibility checks
# pass (i.e., we have decided this is human engagement on a
# ticking-group issue), and before any coalesce / active-run /
# no-pod early returns:
arm_ticker(issue)  # respects user_disabled; resets tick_count,
                    # sets next_run_at, clears disarm_reason.
```

Notes:

- `user_disabled=True` is honored — `arm_ticker` already short-circuits
  for that case, so a user who explicitly turned the ticker off does
  not get it flipped back on by a comment.
- This collapses the practical difference between **Comment** and
  **Comment & Run** for re-arm purposes. Comment & Run still has its
  own value: it forces dispatch of a fresh run even when one is in
  flight or recently completed (today's reset semantics). Plain
  Comment relies on the normal continuation gates (no active run,
  no queued follow-up) to dispatch.
- Symmetric across phases. Applies to In Progress and In Review
  identically — the principle "human engagement restarts automatic
  ticking" is phase-agnostic.
- `noop` / `paused` runs do **not** disarm in the first place
  (§4.4), so re-arm-on-comment is a no-op for them. The behavior
  is meaningful specifically when the prior run emitted `completed`
  or `blocked` and the human is pushing back.

### 4.7 Review-mode inference (no new schema)

"In Review" is polymorphic — what the agent should actually do
depends on what the issue produced. v1 enumerates four kinds:

| Kind               | Trigger signal                                                | Comment surface                | Apply fixes?                     |
| ------------------ | ------------------------------------------------------------- | ------------------------------ | -------------------------------- |
| `CODE`             | impl `done_payload` reports a `pr_url`                        | GitHub PR                      | Yes — commit/push to PR branch   |
| `DESIGN`           | impl `done_payload` reports `design_doc_paths`                | Inline in doc, or pidash issue | Yes — edit doc                   |
| `DESIGN_THEN_CODE` | both `pr_url` AND `design_doc_paths` present                  | Doc first, then PR             | Yes for both                     |
| `GENERIC`          | neither — issue produced something else (research, ops, etc.) | pidash issue                   | Usually no — summary, human acts |

The agent picks the kind at runtime from `parent_done_payload` plus
working-tree inspection (see §5 prompt body). **No new schema.** No
`Issue.review_kind` field in v1. The agent is competent at
artifact inspection and `done_payload` is the canonical
artifact-of-record signal. If field experience shows inference is
unreliable, an explicit `Issue.review_kind` override (default
`auto`) is the v1.5 follow-up — same `PHASES`-style pluggability
as the rest of this design.

The cycle structure (find → validate → comment → apply → resolve →
summary) is the same across kinds; the targets and the
"apply"/"resolve" verbs change. This means: one polymorphic
prompt, one ticker row, one `review` template name in the registry
— all the polymorphism lives in the prompt body, where it should.

This subsection also subsumes what would otherwise need to be a
"GitHub-PR integration" subsection: the agent reads the PR URL
from `done_payload` (no issue↔PR linkage in pidash schema), uses
the runner pod's existing `gh` / git credentials (no new auth
plumbing), and posts the round-trip summary back via the existing
pidash comment API authored by the same `pi_dash_agent` bot
(per Q7).

## 5. Prompt template: `review`

A new global `PromptTemplate` row, `name="review"`,
`workspace=NULL`, `is_active=True`. The template is **polymorphic**
— it routes on what the issue actually produced, because "In Review"
means different things for different issues (see §4.7). Body sketch
(final wording is prompt-engineering work, not design):

```
You are reviewing the work product of a previous implementation pass.
"Review" can mean different things depending on what was produced.

Issue: {{ issue.name }}
Description: {{ issue.description_stripped }}
Recent activity:
{{ comments_section }}
Latest implementation run output (read this carefully —
it is your authoritative record of what was produced):
{{ parent_done_payload }}

Step 1 — Decide what kind of review this is.
Inspect parent_done_payload, the issue description, and the
working tree. Choose ONE:
  (a) CODE — the issue produced a GitHub PR (look for a PR URL in
      done_payload, or a feature branch ahead of main).
  (b) DESIGN — the issue produced a design / planning document
      (look for paths under .ai_design/, doc paths in done_payload,
      or markdown artifacts referenced as outputs).
  (c) DESIGN_THEN_CODE — both a design doc AND a PR exist. Review
      the design first, then the code.
  (d) GENERIC — none of the above. Review the work product against
      the issue description and leave a summary on the pidash issue.

If you cannot decide, ask the human via `paused`.

Step 2 — Run the cycle for the chosen kind. All cycles share this shape:
  i.   Find issues with the work product.
  ii.  Validate your findings (no hallucinations) — re-read the
       artifact, confirm each issue is real, drop any that aren't.
  iii. Read existing reviewer comments (in the PR, in the doc, or
       on the pidash issue depending on kind) and reconcile your
       findings against them.
  iv.  Comment on the validated issues at the appropriate surface:
       - CODE: comments on the GitHub PR (use `gh` CLI).
       - DESIGN: inline comments on the doc, or a structured
         comment on the pidash issue if the doc has no comment
         surface.
       - DESIGN_THEN_CODE: design comments first, then PR comments.
       - GENERIC: a structured comment on the pidash issue.
  v.   If you can fix a confirmed issue and the kind permits it,
       apply the fix and resolve the corresponding comment:
       - CODE: edit, commit, push to the PR branch, resolve the
         PR comment thread.
       - DESIGN: edit the doc and resolve / strike the inline
         comment.
       - GENERIC: usually does NOT auto-apply — leave the summary
         and let the human act.
  vi.  Post a summary back to the pidash issue as a comment:
       confirmed issues found, what you fixed automatically, what
       still needs human action.

Step 3 — Emit a done-signal.
- `completed` = approved, no further automatic ticking needed
  (the review pass is satisfied).
- `blocked` = real issues found that you couldn't auto-fix and
  need human attention.
- `paused` = clarifying question for the human.
- `noop` = nothing has changed since your last review pass.
```

Seed and migration:

- Add `REVIEW_TEMPLATE_BODY` constant alongside the existing
  default in `prompting/seed.py` (or wherever the global default is
  seeded today — verify during impl).
- Data migration: insert the row if it doesn't exist; idempotent.
- Add a reseed command analogous to
  `prompting/management/commands/reseed_default_template.py` for
  operator use during prompt iteration.

### 5.x Implementation-prompt artifact reporting (PR B prerequisite)

The review prompt's mode-detection (Step 1 above) leans on the
**implementation** run's `done_payload` to learn what was produced.
For inference to work reliably, the impl prompt must report any
artifacts it created. PR B should bundle a small update to the
existing `coding-task` template body so it explicitly enumerates
what to put in `done_payload`:

- a `pr_url` field when a PR was created or updated
- a `design_doc_paths` field listing any markdown / planning
  artifacts created (paths relative to repo root)
- the existing `status` and free-form `summary` fields stay
  unchanged

This is a one-line-per-field addition to the impl template, but it
is **load-bearing** for review-mode inference. Without it the
review agent has to fall back to working-tree inspection, which is
slower and noisier.

### 5.1 Why the template only needs to render on the first run

`build_continuation` (`composer.py:67-97`) returns the concatenation
of new human comments since the parent run started, with no template
involvement. That's the path 99% of ticks take. Phase intent does
**not** need to be re-stated each tick — the agent's session memory
already carries the `review` system prompt that landed on the
fresh-session entry (§4.3). Continuation messages are intentionally
minimal: "here is what humans said since you last ran."

The only template-rendered run in In Review is the _first_ one (on
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

### 6.3 New fields on `IssueAgentTicker`

Three additions:

1. **Disarm cause** so deferred auto-pause can distinguish cap-hit
   disarms from terminal-signal disarms.
2. **Resume parent** so leaving Review can explicitly resume the
   pre-review implementation session rather than guessing from the
   latest prior run in a mixed implementation/review ancestry chain.
3. **Per-issue review-phase overrides** so a user who set a fast
   cadence on a hot issue's implementation can tune the review
   cadence for the same issue independently.

```python
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

# Per-issue review-phase overrides. Mirror the existing
# interval_seconds / max_ticks (which now apply to the In Progress
# phase only). NULL means "inherit from project's review default."
review_interval_seconds = models.IntegerField(null=True, blank=True)
review_max_ticks = models.IntegerField(null=True, blank=True)
```

Semantics:

- `interval_seconds` / `max_ticks` are the **In Progress** per-issue
  overrides. (They were "the override" historically; v1 narrows
  the scope to In Progress.)
- `review_interval_seconds` / `review_max_ticks` are the **In
  Review** per-issue overrides.
- `effective_interval_seconds()` and `effective_max_ticks()` become
  **phase-aware** — see §6.4 for the resolution chain.
- `resume_parent_run` is populated on `started -> review` with the
  latest implementation-phase run. On `review -> started`, that run is
  used as the parent for the fresh implementation run; after a
  successful hand-back it is cleared or replaced with the newest
  implementation run.

Backward-compatibility note: the historical meaning of
`interval_seconds` / `max_ticks` ("override for the only ticking
phase") narrows to "override for In Progress." Existing rows
continue to work — those overrides were set while In Progress was
the only phase that ticked, so attributing them to In Progress is
correct.

### 6.4 New project fields + phase-aware effective cadence

`Project` gains two new columns mirroring the existing pair:

```python
# Existing — narrowed in semantic to "In Progress phase defaults"
agent_default_interval_seconds = models.IntegerField(default=10800)  # 3 h
agent_default_max_ticks = models.IntegerField(default=24)            # 3 days

# NEW — In Review phase defaults
agent_review_default_interval_seconds = models.IntegerField(default=10800)  # 3 h
agent_review_default_max_ticks = models.IntegerField(default=8)             # 24 h window
```

`agent_ticking_enabled` is reused as-is — it gates the project
globally regardless of phase.

`IssueAgentTicker.effective_interval_seconds()` and
`effective_max_ticks()` become phase-aware. The resolution chain
selects the In Progress fields when the issue's current state is
the In Progress phase, the Review fields when the state is the In
Review phase, and a sentinel/constant otherwise:

```python
def effective_interval_seconds(self) -> int:
    cfg = phase_config_for(self.issue.state)
    if cfg is None:
        return DEFAULT_INTERVAL_SECONDS  # not currently ticking
    project = self.issue.project
    if cfg.state_name == "In Review":
        override = self.review_interval_seconds
        project_default = getattr(
            project, "agent_review_default_interval_seconds",
            DEFAULT_INTERVAL_SECONDS,
        )
    else:
        override = self.interval_seconds
        project_default = getattr(
            project, "agent_default_interval_seconds",
            DEFAULT_INTERVAL_SECONDS,
        )
    if override is not None and override > 0:
        return override
    return project_default
```

`effective_max_ticks()` mirrors the same shape against
`review_max_ticks` / `agent_review_default_max_ticks` and the
existing impl pair.

Why this shape:

- **Cadence stays on `Project`** — the existing pattern, central
  management preserved.
- **Per-phase split lives in two parallel field pairs** — no
  generic phase axis on `Project`, no JSON, no over-engineering.
  When a third phase arrives later it gets its own pair (e.g.,
  `agent_qa_default_*`).
- **Phase-aware resolver** owns the routing. Other call sites
  (`scheduling.py:arm_ticker`, `bgtasks/agent_ticker.py:fire_tick`,
  scanner annotation) just call `effective_*()` and get the right
  number; they don't need to know which phase is active.
- **Scanner annotation update**: `scan_due_tickers`
  (`bgtasks/agent_ticker.py:42-70`) currently uses `Coalesce(
F("max_ticks"), F("issue__project__agent_default_max_ticks"))`.
  This must become phase-aware too — pick the In Review pair when
  `issue__state__group == "review"`. This is a small `Case/When`
  in the queryset annotation; spelled out in §7.5.

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

- `arm_ticker`: behavior is unchanged from today except that it now
  also clears `disarm_reason` on every arm. It does **not** touch
  `interval_seconds` / `max_ticks`; those remain the user's explicit
  per-issue overrides and are shared across phases. No phase-default
  cadence fields are written, because there are none.
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
  both `from_state` and `to_state` are ticking states in _different_
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
- `handle_issue_comment` (line 209): after the bot-author check
  (line 229) and the eligibility-group check (line 235) pass, call
  `arm_ticker(issue)`. This re-enables automatic ticking after a
  prior `completed`/`blocked` terminal disarm (§4.6). `arm_ticker`
  honors `user_disabled` and is idempotent on an already-armed
  ticker, so the call is safe regardless of prior state. Place the
  call **before** the coalesce / active-run / no-pod early returns
  so re-arm happens for every human comment on an eligible issue,
  not only those that result in immediate dispatch.

### 7.5 `bgtasks/agent_ticker.py`

`fire_tick`: replace `issue.state.name != DELEGATION_STATE_NAME`
(line 112) with `not is_ticking_state(issue.state)`.

`scan_due_tickers` (lines 42-70): the existing scanner uses a
single `Coalesce` to compute the effective cap at the DB level:

```python
effective_cap = Coalesce(
    F("max_ticks"), F("issue__project__agent_default_max_ticks")
)
```

This must become **phase-aware** to respect the cadence split.
Use a `Case/When` over the issue's state group:

```python
from django.db.models import Case, When, F, Q
from django.db.models.functions import Coalesce

effective_cap = Case(
    When(
        issue__state__group=StateGroup.REVIEW.value,
        then=Coalesce(
            F("review_max_ticks"),
            F("issue__project__agent_review_default_max_ticks"),
        ),
    ),
    default=Coalesce(
        F("max_ticks"),
        F("issue__project__agent_default_max_ticks"),
    ),
)
```

The same pattern applies to interval-based filtering if it's added
later. Today's scanner only filters on `next_run_at <= now` and
`tick_count < cap`, so only the cap expression needs the phase
split.

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

Add `REVIEW_TEMPLATE_BODY` constant. New data migration under
`prompting/migrations/` inserts the global row. Also bundle a
small body update to the existing `coding-task` template so impl
runs report `pr_url` and `design_doc_paths` in `done_payload`
(per §5.x — load-bearing for review-mode inference).

## 8. Migrations

Three migrations across the two PRs:

**PR A / M0 — terminal-disarm safety** under `db/migrations/`:

- `AddField` on `IssueAgentTicker.disarm_reason`.
- Backfill existing rows with the empty-string default.

**PR B / M1 — enum + In Review seed + cadence-split fields** under `db/migrations/`:

- `AlterField` on `State.group` to refresh choices.
- `AddField` × 2 on `Project`:
  `agent_review_default_interval_seconds` (default 10800),
  `agent_review_default_max_ticks` (default 8).
- `AddField` × 3 on `IssueAgentTicker`:
  `resume_parent_run`,
  `review_interval_seconds` (null=True),
  `review_max_ticks` (null=True).
- `RunPython` data migration: for every project lacking an In Review
  state, create one in the `review` group, sequence 40000.
- `seeds/data/states.json` and `DEFAULT_STATES` updated alongside.

Backfill notes:

- New `Project` columns get the model-level defaults (10800 / 8)
  for existing rows; no data migration needed.
- New `IssueAgentTicker` override columns are `NULL` on existing
  rows — review uses the project default until a user sets an
  explicit override. Existing `interval_seconds` / `max_ticks`
  values stay attached to the issue, attributed to In Progress per
  §6.3 backward-compat note.

**PR B / M2 — `review` prompt template + `coding-task` body update** under `prompting/migrations/`:

- `RunPython`: insert global
  `PromptTemplate(name="review", workspace=NULL, is_active=True,
body=REVIEW_TEMPLATE_BODY)` if not present. Idempotent.
- `RunPython`: update the existing `coding-task` template body to
  add `pr_url` and `design_doc_paths` reporting fields (§5.x).
  Idempotent — only updates if the existing body lacks the new
  fields.

Both are reversible.

## 9. Tests

New tests:

- `orchestration/test_agent_phases.py` — registry behavior:
  `is_ticking_state`, `phase_config_for`, `template_name_for`,
  exhaustive over every StateGroup value.
- Extend `test_scheduling.py`:
  - `arm_ticker` on In Review uses the **review-phase** effective
    interval/max (`review_interval_seconds` override → project's
    `agent_review_default_*` → constant) and clears `disarm_reason`.
  - `arm_ticker` on In Progress still uses today's `interval_seconds`
    override → `agent_default_*` chain — no regression.
  - Phase-aware `effective_interval_seconds()` /
    `effective_max_ticks()` exhaustively over the four
    override-or-default permutations per phase.
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
    `review` for In Review.
  - `build_continuation` fallback respects the same registry.
- Migration tests:
  - M1 idempotent across re-runs; only In Review rows added.
  - `Project.agent_review_default_*` apply model defaults
    (10800 / 8) to existing rows.
  - `IssueAgentTicker.resume_parent_run` /
    `review_interval_seconds` / `review_max_ticks` backfill as
    `NULL`; existing `interval_seconds` / `max_ticks` values are
    unmutated.
  - M2 inserts the `review` template and updates the `coding-task`
    template body to add `pr_url` / `design_doc_paths` reporting;
    idempotent across re-runs.

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

| #   | Question                                                                                                                              | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| --- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Q1  | Native session resume across phase change?                                                                                            | **No — fresh session on cross-group entry.** §4.3. Code-review system prompt needs to actually be the system prompt, not a user message on a resumed implementation session.                                                                                                                                                                                                                                                                           |
| Q2  | Separate cadence/cap settings for review?                                                                                             | **Yes — split per phase, but kept on `Project` (not in the registry).** §3.2 / §6.4. New columns: `Project.agent_review_default_interval_seconds` (default 10800 = 3 h) and `agent_review_default_max_ticks` (default 8 = 24 h window). New per-issue overrides: `IssueAgentTicker.review_interval_seconds` / `review_max_ticks`. `effective_*()` becomes phase-aware. Existing `agent_default_*` narrows in semantic to "In Progress phase defaults." |
| Q3  | Auto hand-off In Progress → In Review via agent done-signal?                                                                          | **No (v1)** — manual user transition only. Needs `state_transition.requested_group` ingestion (parsed today, never consumed); out of scope.                                                                                                                                                                                                                                                                                                            |
| Q4  | Custom workspace state names within the Review group (e.g., "QA")?                                                                    | **Don't tick (v1)** — same restriction as In Progress today. Generalize to "any state in a ticking group" in a separate change.                                                                                                                                                                                                                                                                                                                        |
| Q5  | Should the review agent be allowed to write code / edit artifacts, or strictly leave comments?                                        | **Allowed to apply confirmed fixes (v1).** §4.7 / §5. The cycle is _find → validate → comment → apply → resolve → summary_. For CODE kind the agent commits/pushes to the PR branch and resolves PR comment threads; for DESIGN kind it edits the doc; for GENERIC it usually only summarizes. The validation step (re-read each finding before acting) is the hallucination guard.                                                                    |
| Q6  | What does Comment & Run on a Paused-from-Review issue resume to?                                                                      | **In Progress (v1)** — unconditional. A phase-aware re-open dialog is the proper fix; ships with the deferred `last_ticking_state` field.                                                                                                                                                                                                                                                                                                              |
| Q7  | Same `pi_dash_agent` bot for review-mode runs, or a separate `pi_dash_reviewer` bot for activity feed UX?                             | **Same bot (v1).** Splitting is a one-line change later.                                                                                                                                                                                                                                                                                                                                                                                               |
| Q8  | Does the new terminal-signal disarm hook also need to fire on In Progress runs (existing gap)?                                        | **Yes.** §4.4 / §7.6. The hook is phase-agnostic; it disarms on true `completed`/`blocked` regardless of which group the issue is in, but it explicitly skips `noop`.                                                                                                                                                                                                                                                                                  |
| Q9  | After a terminal-signal disarm, should a subsequent human comment re-arm the ticker, or stay one-shot?                                | **Re-arm.** §4.6. "Comment is engagement; engagement restarts automatic ticking." Symmetric across In Progress and In Review. `user_disabled=True` is honored and is the only way for a human to stop automatic ticking permanently.                                                                                                                                                                                                                   |
| Q10 | "In Review" can mean code review, design review, both, or generic review depending on the issue. How does the agent know which to do? | **Polymorphic prompt + runtime inference (v1).** §4.7. The agent picks `CODE` / `DESIGN` / `DESIGN_THEN_CODE` / `GENERIC` from the impl run's `done_payload` (`pr_url` and/or `design_doc_paths`) plus working-tree inspection. No new schema. An explicit `Issue.review_kind` override (default `auto`) is the v1.5 follow-up if inference proves unreliable.                                                                                         |

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
- Wire re-arm-on-comment in `handle_issue_comment` (§4.6 / §7.4):
  `arm_ticker(issue)` is called immediately after the bot/group
  eligibility checks pass, before the coalesce/active-run/no-pod
  early returns. This pairs with the terminal disarm so a human
  comment after `completed`/`blocked` restarts automatic ticking.
  `user_disabled=True` continues to be honored.
- Tests for the registry and the new hook.
- This PR is a refactor; ticker behavior for In Progress is
  identical to today _except_ the terminal-signal disarm now
  actually fires (which is what the existing design specified), and
  terminal disarm no longer risks cascading into auto-Pause.

**PR B — Review state group + In Review state + `review` prompt**

Lights up review behavior end to end (code, design, both, or
generic — see §4.7). PR B and the previously-separate prompt PR
are merged: shipping the state without the prompt would render an
implementation prompt against a review-named state, which is
worse than not shipping the state at all.

- M1 migration (StateGroup enum + In Review seed + backfill +
  cadence-split fields per §6.4).
- Update `DEFAULT_STATES` in `state.py`.
- M2 migration (insert `review` template + update existing
  `coding-task` body for `done_payload` artifact reporting per §5.x).
- Add `REVIEW_TEMPLATE_BODY` to `prompting/seed.py`.
- Reseed command analogous to `reseed_default_template.py`.
- Add `REVIEW` entry to `agent_phases.PHASES` with
  `fresh_session_on_entry=True`. No phase-specific cadence; review
  shares `Project.agent_default_*` with In Progress.
- `arm_ticker` clears `disarm_reason` on every arm; nothing else
  changes about cadence-field handling.
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
        is_ticking_state ✓ → arm_ticker (project defaults: 3h × 24),
        immediate dispatch with parent_run=None (no prior run).
        Run R1 created, prompt template "coding-task".
        Ticker: tick_count=0, next_run_at=T+3h+jitter.
T+1h    R1 emits completed → terminal.
        Consumer terminate: maybe_disarm_on_terminal_signal → ticker
        disarmed. (Old behavior: ticker kept ticking. New behavior:
        agent stays quiet until human acts.)
T+2h    User comments "looks good, but please move to review."
        Comment-handler re-arm fires (since the issue is still in
        the started group, eligible). Ticker is re-enabled,
        tick_count=0, fresh next_run_at. A one-shot continuation
        run also dispatches per existing behavior.
T+2h+1m User moves I → In Review.
        from_group=started, to_group=review:
          • disarm rule fires (transient).
          • is_ticking_state(In Review) ✓ → arm_ticker runs the
            phase-aware path: tick_count=0,
            next_run_at=NOW+effective_interval+jitter (3 h from
            agent_review_default_interval_seconds, unless the
            issue's review_interval_seconds is set).
            disarm_reason cleared.
          • Cross-phase fresh-session detected → R2 dispatched with
            parent_run=None and pinned_runner_id cleared.
            ticker.resume_parent_run = R1 (last impl run).
          • R2's first turn renders "review" template as the
            actual system prompt of a fresh agent session. Step 1
            of the prompt detects the review kind from R1's
            done_payload (e.g., CODE if pr_url present).
T+2h+30m  R2 finishes its review, emits paused with a question.
        Consumer terminate: maybe_disarm_on_terminal_signal sees
        non-terminal status (paused) → no-op. Ticker still armed.
T+5h+1m Tick fires for In Review (3 h after the last next_run_at
        reset — review-phase interval, identical to impl interval
        in v1 defaults but separately configurable per §6.4). R3
        created, build_continuation returns concatenated new
        comments since R2's start (including the human's reply to
        R2's question, if any). Native session resume on R2's
        thread — review system prompt persists. Tick count = 2.
...
T+~26h  Cap hit on the In Review ticker (8 ticks × 3 h = 24 h
        review window, smaller than impl's 24 × 3 h = 3 days
        because review-phase max_ticks defaults to 8).
        enabled=false immediately. On the just-fired run's
        termination, maybe_apply_deferred_pause sees:
          • is_ticking_state(I.state) ✓ (In Review)
          • ticker disarmed with disarm_reason=CAP_HIT
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
