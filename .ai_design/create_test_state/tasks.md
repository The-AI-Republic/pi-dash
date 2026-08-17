# In Test State — Implementation Tasks

This file turns the design into a concrete implementation checklist.

Related docs:

- `design.md` — this directory. The full design; do not re-derive it.
- `.ai_design/create_review_state/design.md` — the phase-registry +
  cadence-split + terminal-disarm infrastructure this design extends.
  In Test is the third arm of what review made a binary
  impl-vs-review split. Depend on it; don't re-derive it.

## Suggested rollout

**One PR.** In Test reuses the review scaffold wholesale and adds no
schema columns (the state, enum value, and lifecycle migration all
already ship). Shipping state activation without the `test` prompt
would render a review/implementation prompt against a test-named
state — worse than not shipping. So the registry entry, the cadence
third arm, the cap-hit carve-out, and the prompt land together.

### PR — Activate the In Test phase + `test` prompt

Goal:

- register **In Test** (group `test`) as a ticking phase so moving an
  issue there triggers AgentRuns on the same infrastructure as In
  Review
- give it a **polymorphic `test` prompt** that infers the test kind
  (AUTOMATED / UI / OPS / DESIGN / NON_TECHNICAL) at runtime and posts
  a structured results comment — the answer to "the test can be
  different task by task"

Scope:

- **`apps/api/pi_dash/orchestration/agent_phases.py`** — add the
  third `PHASES` entry:

  ```python
  StateGroup.TEST.value: PhaseConfig(
      state_name="In Test",
      template_name="test",
      fresh_session_on_entry=True,
  ),
  ```

  This alone lights up `_is_delegation_trigger`
  (`service.py:109` → `is_ticking_state`),
  `CONTINUATION_ELIGIBLE_GROUPS` (`service.py:304` →
  `tuple(PHASES.keys())`), `fire_tick`
  (`bgtasks/agent_ticker.py:132`), `template_name_for`, the
  leaving-a-ticking-group disarm, and re-arm-on-comment — all for
  free. No edits needed at those sites.

- **`apps/api/pi_dash/db/models/issue_agent_ticker.py`** — the cadence
  resolvers are binary review-vs-impl and need a test arm:
  - `_is_review_phase()` (`:131`) + `effective_interval_seconds()`
    (`:150`) + `effective_max_ticks()` (`:175`).
  - Preferred: extract a `_phase_cadence_fields(state) ->
(override_attr, project_default_attr)` helper keyed on
    `state.group`, so In Test resolves the test pair. **v1 aliases the
    review pair** (`review_interval_seconds` /
    `agent_review_default_interval_seconds` and the max-ticks pair) —
    no new columns.
  - Acceptable v1 alternative: an explicit `elif In Test` arm if it
    reads more clearly against the existing review branch.

- **`apps/api/pi_dash/orchestration/scheduling.py`**:
  - `_project_default_interval` (`:64`) / `_project_default_max_ticks`
    (`:85`): add a TEST arm returning the test-phase default (v1: the
    `agent_review_default_*` value), or route through the phase-keyed
    lookup.
  - **`maybe_apply_deferred_pause` (`:760`, carve-out at `:799`)** —
    **the one silently-wrong-without-it site.** Today:
    `if state.group == StateGroup.REVIEW.value: return False`. Once
    `TEST` ticks, `is_ticking_state(In Test)` passes at `:789` and a
    cap-exhausted In Test issue would fall through and **auto-move to
    Paused**, contradicting the design's "stays In Test until a human
    moves it." Add the TEST arm — generalize to
    `state.group in (REVIEW, TEST)` or, cleaner, a registry-keyed
    "stay put on cap" check so future phases inherit it. (This is the
    create_review_state design-review's Refinement 1, re-confirmed at
    `@main`.)

- **`apps/api/pi_dash/bgtasks/agent_ticker.py`** —
  `scan_due_tickers` `effective_cap` annotation (`:58`) is
  `Case(When(group==REVIEW …))`. Add a `When(group==TEST …)` arm (v1:
  same `agent_review_default_max_ticks` pair), or refactor the
  annotation to be phase-keyed. `fire_tick` needs no change.

- **`apps/api/pi_dash/orchestration/service.py`** —
  `handle_issue_state_transition` (`:154-224`) cross-phase resume.
  There is **no** `resume_parent_run` _function_ — the logic is inline
  here (this citation was corrected in the create_review_state design
  review). **No production-code change needed** (verified at `@main`,
  third-round review): the capture gate at `:156`
  (`from_state.group == StateGroup.STARTED.value`) fires only on
  leaving In Progress and is the sole `resume_parent_run` write
  (`:174`), so a review→test pass-through leaves the captured impl run
  intact; `arm_ticker`/`disarm_ticker` don't write the field either.
  A later kick-back to In Progress therefore still resumes the impl
  thread via the `fresh_session_on_entry == False` branch (`:211-214`).
  **Do not add "explicit capture" code — it risks introducing the very
  clobber it aims to prevent.** The only deliverable is the regression
  test below. `CONTINUATION_ELIGIBLE_GROUPS` and
  `_is_delegation_trigger` need **no** change — they grow from the
  registry.

- **`apps/api/pi_dash/prompting/recipes.py`** — add
  `KIND_TEST = "test"` and its `RECIPES` entry mirroring
  `KIND_REVIEW`:

  ```python
  KIND_TEST: (
      "test-intro", "session-framing", "pidash-cli",
      "test-cycle", "guardrails", "ending-run",
  ),
  ```

  Confirm `kind_for` maps the `test` template name → `KIND_TEST`.

- **`apps/api/pi_dash/prompting/sections/`** — add `test-intro.md`
  and `test-cycle.md` (analogous to `review-intro.md` /
  `review-cycle.md`). The cycle body: decide kind → uniform test
  cycle (plan → setup → execute → collect evidence → validate
  findings → structured results comment → optional trivial fix /
  follow-up issue) → done-signal. See design §5 for the body sketch.

- **`apps/api/pi_dash/prompting/seed.py`** — add
  `TEST_TEMPLATE_BODY` constant + `seed_test_template()`, analogous to
  `REVIEW_TEMPLATE_BODY` / `seed_review_template`.

- **`apps/api/pi_dash/prompting/sections/state-routing.md`** —
  rewrite the `test` branch (`:14`); it currently says "Automatic
  ticking is **not** wired to this group." Describe the live test
  cycle instead.

- **migration M1** — under `prompting/migrations/`, modeled on
  `0002_review_template.py`: `RunPython` inserting the global
  `PromptTemplate(name="test", workspace=NULL, is_active=True,
body=TEST_TEMPLATE_BODY)` if absent. Idempotent, reversible.

- **migration M2** — under `prompting/migrations/`: `RunPython`
  extending the existing `coding-task` template body so the impl run
  **states the acceptance criteria it validated against in its final
  summary** (→ `parent_done_payload.result`) and/or a durable comment
  (design §4.8) — the test agent's spec, load-bearing for test
  quality. **Correction (third-round review):** this is a prompt-body
  edit, **not** a new structured `done_payload` field — `done_payload`
  is the harness run-result envelope, and no structured `pr_url` /
  `design_doc_paths` artifact field exists to extend. Idempotent (only
  if the body lacks the instruction), reversible. **Verify first:** if
  the impl summary already surfaces acceptance criteria clearly, drop
  M2.

- **`apps/api/pi_dash/prompting/management/commands/reseed_test_template.py`**
  — reseed command analogous to `reseed_review_template.py`.

Tests (design §9):

- `tests/unit/orchestration/test_agent_phases.py`: registry now has
  three entries; `is_ticking_state` / `phase_config_for` /
  `template_name_for` true for In Test; exhaustive over every
  `StateGroup`.
- `test_scheduling.py`:
  - `arm_ticker` on In Test uses the test-phase effective
    interval/max (v1: review default) and clears `disarm_reason`.
  - **`maybe_apply_deferred_pause` does NOT auto-pause a
    cap-exhausted In Test issue** — it stays In Test (§4.5). Regression
    guard: In Progress cap-hit still auto-pauses; In Review still
    stays put.
  - `_project_default_interval` / `_project_default_max_ticks` return
    the test-phase default for an In Test issue.
- `test_issue_agent_ticker.py`: `effective_interval_seconds()` /
  `effective_max_ticks()` resolve the test pair for In Test across the
  override / project-default permutations.
- `test_agent_ticker.py`: `fire_tick` ticks an In Test issue;
  `scan_due_tickers` admits an In Test row below its cap and skips it
  at cap; non-ticking states stay skipped.
- `test_service.py`:
  - `_is_delegation_trigger` true for In Test.
  - In Review → In Test dispatches a fresh session (`parent_run=None`,
    `pinned_runner_id` cleared).
  - the captured implementation `resume_parent_run` survives the
    impl→review→test path; a kick-back to In Progress resumes the
    implementation thread, not a review/test run.
  - comment on In Test wakes the agent.
- `prompting/test_composer.py`: `build_first_turn` selects `test` for
  In Test, `review` for In Review, `coding-task` for In Progress;
  fallback otherwise.
- migration tests: M1 inserts the `test` template idempotently; M2
  updates `coding-task` idempotently (or is dropped).

Why one PR:

- no schema columns → no migration-ordering risk that would justify a
  split
- the cadence third arm, the cap-hit carve-out, and the prompt are
  tightly coupled to the registry entry landing — splitting would
  leave one PR with a ticking state that renders the wrong prompt
- mirrors how the review work concluded that state-without-prompt is
  worse than nothing

## Cross-PR checklist

- [ ] `pnpm check` passes (oxlint + oxfmt + tsc). Backend-heavy, but
      confirm the `test` group is present in the state-group
      enumerations the review work touched
      (`packages/constants/src/state.ts`,
      `apps/api/pi_dash/space/utils/grouper.py`,
      `apps/api/pi_dash/api/views/issue.py`,
      `apps/api/pi_dash/utils/order_queryset.py`) — In Test was
      seeded, so it likely already renders; add any missing arm.
- [ ] `cd apps/api && python -m pytest pi_dash/tests/unit/` passes
- [ ] `cd apps/api && python -m pytest pi_dash/tests/contract/` passes
      — contract tests for the Issue State endpoints may already allow
      the `test` group (seeded); confirm.
- [ ] Reseed the `test` template on a scratch DB and confirm the row
      lands (`seed_test_template()` / the reseed command).
- [ ] Manual smoke: move an issue In Review → In Test and confirm:
      (a) a fresh agent session starts (no `parent_run`,
      `pinned_runner_id` cleared on the new AgentRun row),
      (b) the run logs show the `test` template body as the system
      prompt and the first turn decides a test kind
      (AUTOMATED / UI / OPS / DESIGN / NON_TECHNICAL) per §4.7,
      (c) tick cadence equals the review default (3 h) and the ticker
      stops after the review cap (4) unless overridden,
      (d) the agent posts a **structured results comment** (kind,
      method, per-criterion pass/fail + evidence, defects +
      disposition),
      (e) `completed` disarms the ticker and leaves the issue In Test;
      a follow-up human comment re-arms it (re-arm-on-comment),
      (f) **cap exhaustion leaves the issue In Test — NOT Paused**
      (the §4.5 carve-out),
      (g) for an AUTOMATED-kind run (issue with a PR), the agent ran
      the repo's gates, pushed any trivial fix to the PR branch,
      and reported real defects as `blocked` / a follow-up issue
      rather than a false pass,
      (h) moving the issue back to In Progress resumes the stored
      pre-review **implementation** thread, not the review or test
      run (the impl→review→test `resume_parent_run` invariant).
- [ ] Open questions in design §11 are pinned before ship
      (especially Q6 — the UI/e2e capability gap — and Q8 — where
      acceptance criteria come from).
