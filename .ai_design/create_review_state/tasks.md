# In Review State — Implementation Tasks

This file turns the design into a concrete implementation checklist.

Related docs:

- `design.md` — revised after design review; implementation-readiness
  gaps resolved.
- `.ai_design/issue_ticking_system/design.md` — the upstream ticking infra
  this design extends (do not re-derive it; depend on it).

## Suggested rollout

Two PRs. The earlier draft proposed three (registry refactor → state +
schema → prompt template); design review concluded that shipping the
state without the prompt actively misleads users (an issue moved into
In Review would render the implementation prompt against a review-
named state). PRs 2 and 3 are merged.

### PR A — Phase registry refactor + terminal-signal disarm hook

Goal:

- replace hard-coded `"In Progress"` constants with a single
  registry, so adding the next ticking phase is a registry edit
  instead of a multi-file hunt
- close a gap in the existing issue-ticking system: terminal
  `completed`/`blocked` should disarm the ticker but currently
  doesn't (verified — `disarm_ticker` is only called from the
  state-transition handler at `service.py:123`; the runner-consumer
  terminate paths only call `maybe_apply_deferred_pause`, which
  needs a _pre-disarmed_ ticker to act)

Scope:

- create `apps/api/pi_dash/orchestration/agent_phases.py` with:
  - `@dataclass(frozen=True) class PhaseConfig` — `state_name`,
    `template_name`, `fresh_session_on_entry`,
    `disarm_on_completed` (no per-phase cadence in v1; both phases
    share `Project.agent_default_interval_seconds` /
    `agent_default_max_ticks` and any per-issue override on
    `IssueAgentTicker`)
  - `PHASES: dict[str, PhaseConfig]` — seeded with only the
    `started` entry in this PR (current values: `state_name="In Progress"`,
    `template_name=PromptTemplate.DEFAULT_NAME`,
    `fresh_session_on_entry=False`)
  - `is_ticking_state(state) -> bool`
  - `phase_config_for(state) -> Optional[PhaseConfig]`
  - `template_name_for(state) -> str`
- replace literal `"In Progress"` checks in:
  - `orchestration/service.py:_is_delegation_trigger` →
    `is_ticking_state(to_state)`
  - `orchestration/service.py:handle_issue_state_transition` disarm rule
    (lines 118-123) → "leaving a ticking group disarms"
  - `orchestration/scheduling.py:maybe_apply_deferred_pause` (lines
    320-323) → `is_ticking_state(state)`
  - `bgtasks/agent_ticker.py:fire_tick` (line 112) →
    `not is_ticking_state(issue.state)`
- thread `template_name_for(issue.state)` through
  `prompting/composer.py:build_first_turn` and the fallback inside
  `build_continuation`
- add `maybe_disarm_on_terminal_signal(run)` to
  `orchestration/scheduling.py`:
  - inspect `run.done_payload["status"]`
  - disarm only on `completed` / `blocked`, never on `noop`
  - idempotent; safe to call alongside `maybe_apply_deferred_pause`
- migration M0 — under `db/migrations/`:
  - `AddField` on `IssueAgentTicker.disarm_reason`
  - backfill existing rows with empty-string default
- wire the new hook into both runner-consumer terminate paths
  (`runner/consumers.py:_handle_run_paused` ~line 636 and
  `_finalize_run` ~line 708):
  - call `maybe_disarm_on_terminal_signal` **before**
    `maybe_apply_deferred_pause` (so the deferred-pause hook sees the
    latest disarm reason)
  - same `transaction.on_commit` pattern, same exception-swallowing
    wrapper as the existing call to `maybe_apply_deferred_pause`
- add re-arm-on-comment to `orchestration/service.py:handle_issue_comment`
  (~line 209): immediately after the bot-author check (line 229) and
  the eligibility-group check (line 235) pass, call
  `arm_ticker(issue)`. Place the call **before** the
  coalesce / active-run / no-pod early returns so re-arm fires for
  every human comment on an eligible issue. `arm_ticker` honors
  `user_disabled` and is idempotent on an already-armed ticker.
- add `apps/api/pi_dash/tests/unit/orchestration/test_agent_phases.py`
  exhaustively over every `StateGroup` value
- extend `test_scheduling.py` with:
  - `maybe_disarm_on_terminal_signal` disarms on done-payload
    `completed` and `blocked`, no-ops on `noop` / `paused`,
    idempotent
  - `maybe_apply_deferred_pause` requires
    `disarm_reason == CAP_HIT`
  - terminal-signal terminate does **not** auto-Pause merely because
    the ticker is now disabled
- add migration coverage for M0:
  - existing ticker rows backfill `disarm_reason=""`
- regression-check existing tests for `_is_delegation_trigger`,
  `fire_tick`, `maybe_apply_deferred_pause`, and the composer — they
  should all still pass without semantic change. **The terminal-
  signal disarm is a behavior change for In Progress runs; tests
  asserting that a terminal `completed`/`blocked` leaves the ticker
  armed will need to flip.** This is intentional — the existing
  design specifies this behavior; the implementation gap is being
  closed.
- add tests for re-arm-on-comment in
  `tests/unit/orchestration/test_service.py`:
  - human comment on a disarmed ticker (`disarm_reason=TERMINAL_SIGNAL`)
    flips `enabled=True`, `tick_count=0`, sets `next_run_at`, and
    clears `disarm_reason` — even when the comment path returns
    "prior-run-active" or "coalesced" without dispatching
  - bot comment does **not** re-arm (early return on
    `actor.is_bot`)
  - comment on a non-eligible state group does **not** re-arm
    (early return on `state-not-eligible`)
  - `user_disabled=True` ticker stays disabled across a comment
    (the explicit user kill switch is preserved)

Why first:

- registry seed is single-phase, so visible behavior is unchanged
  except for the terminal-signal disarm fix
- prerequisite for PR B (which adds a registry entry and uses
  `phase_config_for` for cross-phase fresh-session logic)
- ships independently as a refactor + bug fix; reviewable in
  isolation

### PR B — Review state group + In Review + `review` prompt

Goal:

- introduce the `Review` state group, the `In Review` state in every
  project, ticking on it, and the polymorphic `review` prompt
  (code / design / design+code / generic — see design §4.7) — end
  to end

Scope:

- `apps/api/pi_dash/db/models/state.py`:
  - add `REVIEW = "review", "Review"` to `StateGroup`
  - add `In Review` entry to `DEFAULT_STATES` (sequence 40000,
    color: indigo `#5B5BD6` — final color picked during impl)
- `apps/api/pi_dash/seeds/data/states.json`: add the `In Review`
  entry alongside the in-Python default
- `apps/api/pi_dash/db/models/project.py`:
  - add `agent_review_default_interval_seconds` (default 10800)
  - add `agent_review_default_max_ticks` (default 8)
- `apps/api/pi_dash/db/models/issue_agent_ticker.py`:
  - add `review_interval_seconds` (`null=True`, `blank=True`)
  - add `review_max_ticks` (`null=True`, `blank=True`)
  - rewrite `effective_interval_seconds()` and
    `effective_max_ticks()` to be **phase-aware** (consult
    `phase_config_for(self.issue.state)`; pick `review_*` fields +
    `agent_review_default_*` for the review phase, `interval_seconds`
    / `max_ticks` + `agent_default_*` for the In Progress phase)
- migration M1 — `0NNN_review_state.py` under `db/migrations/`:
  - `AlterField` on `State.group` to refresh choices
  - `AddField` × 2 on `Project`:
    `agent_review_default_interval_seconds`,
    `agent_review_default_max_ticks`
  - `AddField` × 3 on `IssueAgentTicker`:
    `resume_parent_run`,
    `review_interval_seconds`,
    `review_max_ticks`
  - `RunPython` data migration: for every project lacking an
    `In Review` state in the `review` group, create one (idempotent)
- `apps/api/pi_dash/prompting/seed.py`: add
  `REVIEW_TEMPLATE_BODY` constant (verify location vs. the
  existing default during impl); also update
  `CODING_TASK_TEMPLATE_BODY` (or wherever the impl default lives)
  to enumerate `pr_url` and `design_doc_paths` reporting in the
  agent's `done_payload` — load-bearing for review-mode inference
  per design §5.x
- migration M2 — under `prompting/migrations/`:
  - `RunPython`: insert a global
    `PromptTemplate(name="review", workspace=NULL,
is_active=True, body=REVIEW_TEMPLATE_BODY)` if not present
    (idempotent)
  - `RunPython`: update the existing `coding-task` template body to
    include the new artifact-reporting fields. Idempotent — only
    update if existing body lacks the new fields
- `apps/api/pi_dash/prompting/management/commands/reseed_review_template.py`:
  reseed command analogous to `reseed_default_template.py`
- `orchestration/agent_phases.py`: add the Review entry to `PHASES`:
  `state_name="In Review"`, `template_name="review"`,
  `fresh_session_on_entry=True`. **No** cadence on `PhaseConfig` —
  cadence stays on `Project` per design §3.2 / §6.4
- `orchestration/scheduling.py:arm_ticker`:
  - clear `disarm_reason` on re-arm
  - do **not** touch override fields; those are still
    user-configured per-phase (`interval_seconds` / `max_ticks` for
    In Progress, `review_interval_seconds` / `review_max_ticks` for
    In Review)
  - effective interval/max come from the **phase-aware**
    `effective_*()` methods on the ticker (issue's review override
    → project's review default → constant when in review;
    similarly for In Progress)
- `bgtasks/agent_ticker.py:scan_due_tickers`:
  - rewrite the `effective_cap` annotation to be phase-aware via
    `Case/When` over `issue__state__group` (design §7.5). Pick the
    `review_max_ticks` / `agent_review_default_max_ticks` pair when
    the issue is in the review group; default to the existing pair
    otherwise
- `orchestration/service.py`:
  - extend `handle_issue_state_transition` to detect cross-phase
    transitions (both `from_state` and `to_state` are ticking
    states in _different_ groups) and pass a `fresh_session=True`
    flag down to `_create_and_dispatch_run`
  - `_create_and_dispatch_run`: when `fresh_session=True`, dispatch
    with `parent_run=None` and clear `pinned_runner_id`; otherwise
    today's parent-resolution logic
  - on `In Progress -> In Review`, capture the latest
    implementation-phase run into `ticker.resume_parent_run`
  - on `In Review -> In Progress`, use
    `ticker.resume_parent_run` as the new run's parent instead of the
    latest prior review run
  - derive pinned-runner restoration from that same explicit parent
- `orchestration/scheduling.py`:
  - persist `disarm_reason=CAP_HIT` on cap exhaustion
  - persist `disarm_reason=TERMINAL_SIGNAL` on true
    `completed` / `blocked`
  - gate `maybe_apply_deferred_pause` on
    `disarm_reason == CAP_HIT`
- `orchestration/service.py`:
  - **expand `CONTINUATION_ELIGIBLE_GROUPS` (line 206) from
    `(StateGroup.STARTED.value,)` to `tuple(PHASES.keys())`** — without
    this, comments on In Review issues never wake the agent (this is
    the reviewer's blocking finding #2)
- tests per design §9:
  - `test_scheduling.py`:
    - arm on In Review clears `disarm_reason` and yields the
      review-phase effective interval/max
      (`review_interval_seconds` override → project's
      `agent_review_default_*` → constant)
    - arm on In Progress still uses today's `interval_seconds`
      override → `agent_default_*` chain — no regression
    - cross-group transition In Progress → In Review re-arms
      cleanly with the new effective values
    - deferred pause works on In Review when cap is hit at the
      review-phase cap (default 8)
    - `maybe_disarm_on_terminal_signal` works on In Review runs
  - `test_issue_agent_ticker.py` (or wherever the model lives):
    `effective_interval_seconds()` and `effective_max_ticks()`
    exhaustively over the four override-or-default permutations
    per phase (override set / project default only) × (In Progress
    / In Review)
  - `test_agent_ticker.py`: `fire_tick` ticks an In Review issue;
    `scan_due_tickers` admits an In Review row when its
    `tick_count < agent_review_default_max_ticks` and skips when
    it reaches that cap; non-ticking states stay skipped
  - `test_service.py`: `_is_delegation_trigger` true for In Review;
    disarm rule triggered by leaving Review group; In Progress →
    In Review dispatches with `parent_run=None`; In Review →
    In Progress dispatches with `ticker.resume_parent_run` as
    parent; comment on In Review wakes the agent
  - `prompting/test_composer.py` (extend): `build_first_turn`
    selects `coding-task` for In Progress, `review` for In Review,
    falls back to default for any other group
- migration tests:
  - M1 idempotent across re-runs; only In Review rows added
  - `Project.agent_review_default_*` apply model defaults
    (10800 / 8) to existing rows
  - `IssueAgentTicker.resume_parent_run` /
    `review_interval_seconds` / `review_max_ticks` backfill as
    `NULL`; existing `interval_seconds` / `max_ticks` values are
    unmutated and continue to apply to In Progress
  - M2 inserts the `review` template and updates the `coding-task`
    template body to add `pr_url` / `design_doc_paths` reporting;
    idempotent across re-runs

Why this PR is intentionally chunky:

- the design review concluded that shipping the state without the
  prompt is worse than shipping nothing — the moment In Review
  appears on the board, a user dragging an issue into it gets the
  implementation prompt rendered against a review-named state
- the cadence split (Project + ticker fields, phase-aware
  `effective_*()`, scanner annotation update) is tightly coupled
  to the review phase landing — splitting them would leave one PR
  with code that has no caller and another PR with a caller that
  has no code
- merging the previously-split lifecycle and prompt PRs avoids that
  intermediate state and makes the rollout boundary mean what it
  says

### Optional PR C — UI polish + phase-aware re-open dialog

Defer unless impl pressure allows:

- Add `last_ticking_state` FK to `IssueAgentTicker` (the field
  dropped from v1 per design §6.3 — it ships when its consumer
  ships)
- Wire `arm_ticker` to record `last_ticking_state` on every arm
- Comment & Run confirmation dialog on a Paused issue reads
  `ticker.last_ticking_state` and shows "resume code review" vs.
  "resume implementation" copy; on Confirm, transitions to the
  recorded state instead of unconditionally to In Progress
- Cap-hit copy on a Paused-from-review issue mentions "code review"
  rather than "agent"
- Project state-management view confirms the new Review column
  renders correctly after updating hard-coded group lists in:
  `packages/constants/src/state.ts`,
  `apps/api/pi_dash/space/utils/grouper.py`,
  `apps/api/pi_dash/api/views/issue.py`, and
  `apps/api/pi_dash/utils/order_queryset.py`

## Cross-PR checklist

- [ ] `pnpm check` passes (oxlint + oxfmt + tsc) — backend changes only,
      but the JS surface is **not** actually untouched: the project
      state constants/types and ordering helpers must learn about the
      new `review` group
- [ ] `cd apps/api && python -m pytest pi_dash/tests/unit/` passes
- [ ] `cd apps/api && python -m pytest pi_dash/tests/contract/` passes —
      contract tests for the Issue State endpoints may need to allow the
      new `review` group value
- [ ] Manual smoke (after PR A): create a project, move an issue
      Todo → In Progress, let the agent run a turn, confirm that on
      `completed` the ticker actually disarms (regression of the gap
      PR A closes), confirm that a `noop` turn does **not** disarm
      it, and confirm that a follow-up human comment after a
      `completed`-disarmed run flips `enabled` back to True
      (re-arm-on-comment) — the issue should resume automatic
      ticking on its next `next_run_at`
- [ ] Manual smoke (after PR B): move the same issue In Progress →
      In Review and confirm:
      (a) a fresh agent session starts (no `parent_run`,
      `pinned_runner_id` cleared on the new AgentRun row),
      (b) the run logs show the `review` template body as the
      system prompt and the agent's first turn explicitly
      decides a review kind (CODE / DESIGN / DESIGN*THEN_CODE
      / GENERIC) per design §4.7,
      (c) review tick cadence equals the project's
      `agent_review_default_interval_seconds` (3 h) and the
      ticker stops automatically after
      `agent_review_default_max_ticks` (8) ticks unless a
      `review*_`per-issue override is set,
    (d) commenting on the In Review issue triggers a continuation
        run within a few seconds,
    (e) emitting`completed` from the review run disarms the
        ticker and leaves the issue in In Review for the human
        to transition forward; a follow-up human comment re-arms
        the ticker (re-arm-on-comment),
    (f) moving the issue back to In Progress resumes the stored
        pre-review implementation thread rather than parenting off
        the latest review run, and the In Progress cadence
        fields (`agent*default*_`/ `interval_seconds`/
       `max_ticks`) take effect again,
    (g) any pre-existing impl-phase override
        (`interval_seconds`/`max_ticks`) survives the phase
        round-trip unchanged; review-phase overrides
        (`review_interval_seconds`/`review_max_ticks`) are
      independent and only apply while the issue is In Review,
      (h) for a CODE-kind review run (issue with a PR), the smoke
      confirms the agent commented on the GitHub PR, applied
      a confirmed fix as a commit on the PR branch, resolved
      the corresponding PR comment thread, and posted a
      summary back as a pidash issue comment
- [ ] Open questions in design §11 are pinned before PR B ships
      (especially Q5 — review agent's write boundary — and Q10 —
      mode inference reliability)
