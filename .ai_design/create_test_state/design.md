# In Test State — Testing/QA Ticking

> Directory: `.ai_design/create_test_state/`
>
> **Status:** ready for implementation. This design is the direct
> sequel to `.ai_design/create_review_state/` and reuses its
> machinery almost wholesale. The review work already turned the
> ticker into an extensible **phase registry**
> (`orchestration/agent_phases.py:PHASES`) and split cadence per
> phase on `Project`. Adding **In Test** as a 6th ticking phase is
> "one registry entry + one polymorphic prompt + a third arm at
> each cadence/pause/resume site that review made phase-aware."
>
> Decisions resolved up front (mirroring the review precedent):
> fresh session on cross-group entry (§4.3), terminal-signal disarm
> reuses the existing hook (§4.4), re-arm on human comment is
> already registry-symmetric (§4.6), polymorphic `test` prompt with
> runtime kind inference — AUTOMATED / UI / OPS / DESIGN /
> NON*TECHNICAL (§4.7 / §5). **Cadence:** v1 reuses the **review**
> defaults (3 h × 4) rather than introducing a third `Project`
> field pair — a stale test window is as worthless as a stale
> review window, and adding `agent_test_default*\*` is a trivial
> follow-up if the rhythm needs to diverge (§3.2).
>
> **Scope:** wire the already-seeded **In Test** state (group
> `test`) into the AgentRun ticking system so that moving an issue
> there wakes the agent on the same tick infrastructure as In
> Review, but with a **`test`** prompt that drives _testing / QA_
> of the issue's work product and reports results as issue
> comments. Because "testing" differs wildly per issue (frontend,
> backend, ops, design, non-technical), the prompt is **polymorphic
> and infers the test kind at runtime** — the same answer the
> `review` prompt gives to "every task reviews differently."
>
> **What this changes about today's code**
>
> The review design (`.ai_design/create_review_state/`) shipped a
> `(group, state-name) → PhaseConfig` registry, phase-aware cadence
> on `Project` (`agent_review_default_*`), a `disarm_reason` field,
> a terminal-signal disarm hook, and a phase-aware `effective_*()`
> resolver. Crucially, that code generalized the _binary_ "impl vs
> review" split — several call sites still branch
> `state.group == REVIEW` explicitly. This design adds the **third
> arm** (`test`) at each of those binary sites, plus the `test`
> prompt kind/recipe/template/seed. No new lifecycle machinery is
> invented.

## 1. Problem

The **In Test** state exists (`db/models/state.py:19`
`TEST = "test", "Test"`; seeded via migration
`0154_test_state_group.py`, sequence between In Review and Done)
but is **dead weight in the orchestration layer**: moving an issue
into it never wakes an agent. That migration's own docstring spells
out the deliberate omission — no `PhaseConfig` entry is added, so
issues in In Test do not auto-tick. This issue is exactly that gap.

The state-routing prompt section even documents the dead end today
(`prompting/sections/state-routing.md:14`):

> `test` — the issue is parked in the **In Test** state (group
> `test`) for testing / QA. Automatic ticking is **not** wired to
> this group, so you should not normally be invoked here …

Two questions to answer:

1. **How does moving an issue to In Test trigger AgentRuns?**
2. **How do we prompt an agent to actually _test_ an issue's work
   product** — given that the test varies task-by-task — and get
   the result back as an issue comment?

The tension the issue calls out is real: some tasks are frontend
(easy to eyeball), some backend (run the suite), some ops (validate
config), some design (check a doc against criteria), some entirely
non-technical. We do **not** want to build or wire a traditional
per-task test pipeline. The review phase already faced the exact
same "every task is different" problem and solved it with one
polymorphic prompt that picks the right cycle at runtime. In Test is
that same idea with test-flavored kinds.

## 2. Goal

Add a **test** phase that the agent drives autonomously on a ticking
cadence, distinct from implementation and review. "Testing" is
polymorphic — the agent picks the right cycle (run the repo's
suites, drive a UI flow, validate ops config, check a design/doc
against acceptance criteria, or verify a non-technical deliverable
against a checklist) from the work product itself; see §4.7 for the
inference rules.

- Register **In Test** (group `test`) as a ticking phase, parallel
  to In Review. No new state, enum value, or lifecycle migration —
  all already seeded.
- Periodic ticking on In Test uses the same ticker primitives as In
  Review (same row, same scanner, same cap path, same Comment & Run
  reset semantics), with cadence that **reuses the review defaults**
  in v1 (3 h × 4 — a 12 h test window). See §3.2.
- The test run uses a new **`test`** prompt template that routes
  between AUTOMATED / UI / OPS / DESIGN / NON_TECHNICAL testing
  based on what the issue produced (§4.7 / §5).
- The test run starts on a **fresh agent session**
  (`fresh_session_on_entry=True`), so the `test` template body lands
  as the actual system prompt rather than as a user message on a
  resumed implementation/review session.
- Terminal `completed` / `blocked` signals disarm the ticker via the
  **existing** `maybe_disarm_on_terminal_signal` hook — no new hook.

Non-goals (v1):

- **Auto-transition In Review → In Test** on a review done-signal.
  v1 uses manual user transitions only — matching how In Review is
  entered today. A done-signal-driven hand-off is a follow-up.
- **Browser / e2e capability in the runner.** The UI kind (§4.7)
  needs a headless browser + the ability to boot the app in the
  pod; that is a runner-capability follow-up, not a blocker for the
  phase. v1 leans on what the runner already has (git, `gh`, the
  repo toolchain) and treats UI testing as best-effort /
  `paused`-if-impossible.
- **A dedicated `Project.agent_test_default_*` cadence pair.** v1
  reuses the review defaults; the split is a one-migration follow-up
  if test rhythm needs to diverge from review.
- **Generalization to "any state in any group ticks."** v1 keeps the
  per-group designated-state-name model: literal `"In Test"` in the
  `test` group. Same constraint as In Progress / In Review.

## 3. Design — reuse the phase registry, add the third arm

The review design replaced hard-coded `"In Progress"` strings with a
`(group, state-name) → PhaseConfig` registry
(`orchestration/agent_phases.py`). That registry currently holds two
entries:

```python
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
```

The core of this design is a **third entry**:

```python
    StateGroup.TEST.value: PhaseConfig(
        state_name="In Test",
        template_name="test",
        fresh_session_on_entry=True,   # the test system prompt must
                                       # land as the actual system prompt
    ),
```

Because the trigger, comment-wake, and template-selection paths are
**already registry-driven**, that single entry lights up most of the
phase for free (verified at `@main`):

- `service.py:109` `_is_delegation_trigger(to_state)` →
  `is_ticking_state(to_state)`. Moving an issue to a registered
  ticking state dispatches a run. In Test now hits.
- `service.py:304` `CONTINUATION_ELIGIBLE_GROUPS = tuple(PHASES.keys())`.
  Comments on In Test issues now wake the agent.
- `bgtasks/agent_ticker.py:132` `fire_tick` gates on
  `not is_ticking_state(issue.state)`. In Test ticks now fire.
- `prompting/composer.py` `template_name_for(state)` selects the
  `test` template for the first render.
- The "leaving a ticking group disarms" rule and re-arm-on-comment
  (§4.6) are already phase-agnostic — In Test inherits both.

### 3.1 The spots that are NOT free — a third arm at each binary site

Review generalized _impl → binary(impl, review)_, but several call
sites still branch on `state.group == StateGroup.REVIEW.value`
explicitly rather than looping over the registry. Each needs a
`TEST` arm (or generalization to a phase-keyed lookup). These are
the honest "it's not purely one line" touchpoints, all verified at
`@main`:

| Site                                                                                                                              | Current shape                                                | What TEST needs                                                                                                         |
| --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------- |
| `db/models/issue_agent_ticker.py:131` `_is_review_phase()` + `effective_interval_seconds`/`effective_max_ticks` (`:150` / `:175`) | binary: review pair vs impl pair                             | resolve the **test** cadence pair (v1: reuse review's — §3.2 / §6)                                                      |
| `orchestration/scheduling.py:64` `_project_default_interval` / `:85` `_project_default_max_ticks`                                 | `if state.group == REVIEW: review default else impl default` | a TEST arm returning the test-phase default (v1: the review default)                                                    |
| `bgtasks/agent_ticker.py:58` `scan_due_tickers` `effective_cap` `Case(When(group==REVIEW …))`                                     | binary DB-level annotation                                   | a TEST `When` (v1: same `agent_review_default_max_ticks` pair)                                                          |
| `orchestration/scheduling.py:799` `maybe_apply_deferred_pause` REVIEW carve-out                                                   | `if state.group == REVIEW: return False` (no auto-pause)     | **a TEST arm** — see §4.5. This is the one spot where "just add the registry entry" produces silently _wrong_ behavior. |
| `orchestration/service.py:154-224` `handle_issue_state_transition` cross-phase resume                                             | assumes a two-phase impl↔review chain                        | be explicit about which prior run a test run resumes/reads from in a 3-phase impl→review→test flow (§4.3)               |
| `prompting/sections/state-routing.md:14` `test` branch prose                                                                      | "Automatic ticking is not wired to this group"               | rewrite to describe the live test cycle                                                                                 |

None are large. The cleanest implementation generalizes the binary
`_is_review_phase()` / `Case-When(REVIEW)` idioms into a small
phase-keyed cadence lookup keyed on `state.group`, so a future
`qa`/whatever phase is a registry edit rather than another
three-site hunt. v1 may keep the explicit third arm if that reads
more clearly against the existing review code; see §6.

### 3.2 Cadence: reuse the review defaults (v1)

Cadence stays centrally managed on `Project` (row + per-issue
override) — the pattern review established. v1 does **not** add a
third field pair. In Test resolves to the **review** cadence
defaults:

- `agent_review_default_interval_seconds` = 10800 (3 h)
- `agent_review_default_max_ticks` = 4 (a 12 h test window)

Rationale:

- A test that sits open for days has gone stale, exactly like a
  review — the short cap fits.
- The 3 h interval is the human-involvement window: each tick is a
  complete test cycle, then the agent stops to leave room for a
  human to look at the results.
- Adding `Project.agent_test_default_interval_seconds` /
  `agent_test_default_max_ticks` (+ `IssueAgentTicker.test_*`
  overrides) is a mechanical follow-up if field experience shows the
  test rhythm should diverge from review. Keeping v1 to two field
  pairs avoids a schema change for a value we'd only be guessing at.

The per-issue override story: In Test reuses `review_interval_seconds`
/ `review_max_ticks` in v1 (they resolve through the same review
default). If/when a dedicated pair lands, it splits cleanly the same
way review split from impl.

## 4. Lifecycle

### 4.1 State group already exists

No new `StateGroup` value, no new `State` row, no lifecycle
migration. `StateGroup.TEST = "test"` and the **In Test** state are
already seeded (migration `0154_test_state_group.py`, sequence
between In Review 40000-ish and Done). This design only _activates_
the phase.

### 4.2 Lifecycle parity with In Review

Treat **In Test** as a sibling of In Review with respect to the
ticker lifecycle. Every touchpoint the review design wired works
unchanged once the registry has the TEST entry and the cadence
resolvers grow their third arm:

| Event                                            | Behavior                                                                                                                                                               |
| ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Issue enters In Test                             | `_is_delegation_trigger` returns True (registry hit) → `arm_ticker` (test-phase effective cadence = review default in v1) + immediate dispatch on a **fresh session**. |
| Periodic tick on In Test                         | `fire_tick` registry-checks, claims, dispatches — same atomic-claim flow as In Review.                                                                                 |
| Issue leaves the Test group                      | `disarm_ticker` — the phase-agnostic "leaving a ticking group disarms" rule.                                                                                           |
| Cap hit during In Test                           | Disarm + **stay In Test** (no auto-pause). Requires the TEST arm in `maybe_apply_deferred_pause` (§4.5).                                                               |
| Agent emits `completed` / `blocked`              | Existing `maybe_disarm_on_terminal_signal` hook disarms. Issue stays In Test until a human transitions it.                                                             |
| Comment on In Test (incl. Comment & Run)         | Triggers continuation **and** re-arms the ticker (§4.6) — `CONTINUATION_ELIGIBLE_GROUPS` already includes `test` via `tuple(PHASES.keys())`.                           |
| Comment & Run on a Paused issue formerly in Test | Re-opens to In Progress (v1 default — same as review).                                                                                                                 |

### 4.3 Cross-group transition: fresh session + explicit resume

User moves an issue **In Review → In Test** (the expected path):

1. From-group `review`, to-group `test`.
2. "Leaving a ticking group disarms" fires `disarm_ticker(issue)`
   (transient — re-armed in step 3).
3. `_is_delegation_trigger(in_test_state)` returns True via the
   registry → `arm_ticker` on the test-phase effective cadence.
4. `phase_config_for(state).fresh_session_on_entry` is True for the
   test phase → dispatch the first In Test run with `parent_run=None`
   and `pinned_runner_id` cleared. The `test` template body becomes
   the system prompt of a fresh session.

The fresh-session choice is load-bearing for the same reason it was
in review: mid-conversation re-roling ("you were reviewing; now
test") is unreliable. Starting fresh costs the prior context (the
agent re-reads the issue / `done_payload` / diff) but makes the phase
signal unambiguous and lets the `test` template do its job. As in
review, only the _first_ In Test run renders the template; every
subsequent tick is comment-delta only (`build_continuation`), and the
`test` system prompt persists in the session's memory.

**Three-phase resume (verified: no production-code change needed —
add a regression test).** The review design captured an explicit
`ticker.resume_parent_run` so that In Review → In Progress resumes
the _implementation_ session rather than the latest (review) run.
With a third phase, the concern is: does the captured implementation
resume target survive the _pass-through_ review→test, so a later
kick-back to In Progress still resumes the impl thread? **A
third-round review at `@main` confirmed the existing code already
does this correctly with no change** (the resume logic is inline in
`handle_issue_state_transition`, `service.py:154-224`, **not** a
named `resume_parent_run` function — that citation was corrected in
the earlier design review). Two facts settle it:

1. The only production write to `resume_parent_run` is
   `service.py:174`, gated by `service.py:156`
   (`cross_phase and from_state.group == StateGroup.STARTED.value`).
   So the capture fires **only** on leaving In Progress. A review→test
   transition has `from_state.group == "review"`, so line 174 does
   **not** run and the stored impl run is left untouched.
2. Neither `arm_ticker` nor `disarm_ticker` writes `resume_parent_run`
   (their `update_fields` cover only the clock / `enabled` /
   `disarm_reason`; the create-path defaults don't set it). The
   disarm-then-re-arm the pass-through performs therefore preserves it.

Traced end to end: In Progress → In Review captures `resume_parent_run
= R_impl` (line 174). In Review → In Test leaves it intact (line 174
skipped; arm/disarm don't touch it). In Test → In Progress hits the
`fresh_session_on_entry == False` branch (`service.py:211-214`), reads
`ticker.resume_parent_run` = R*impl, and resumes the implementation
thread. **So §7.4 needs no new resume logic — only a regression test
(§9) locking in that the capture survives review→test.** Actively
"making the capture explicit" would risk \_introducing* the clobber
this paragraph warns against; leave the gate as-is.

For mode inference on entering In Test (§5, Step 1), the test run
reads the artifact-of-record from the **implementation** run — but via
the same channel review uses today: the run's free-text result
(surfaced to the next phase as `parent_done_payload`) plus
working-tree / run-history inspection, **not** a structured artifact
manifest (see §4.8 — there is no structured `pr_url` /
`design_doc_paths` field in `done_payload`; that is the harness
run-result envelope). The review run's payload is a verdict, not an
artifact manifest, so the test prompt should key its kind inference
off the working tree (feature branch ahead of `main`, `.ai_design/`
paths) exactly as `review-cycle.md` does. No new FK, no clobber, no
new logic — just the regression test.

### 4.4 Done-signal handling (reuses the review hook)

The four agent done-signal statuses keep their terminal semantics.
The **existing** `maybe_disarm_on_terminal_signal(run)`
(`orchestration/scheduling.py`) is phase-agnostic — it inspects
`run.done_payload["status"]` and disarms on `completed` / `blocked`,
never on `noop`. In Test inherits it with no change.

Signal meanings, test-flavored:

- `completed` = all tests pass / every acceptance criterion met —
  testing satisfied. Issue stays In Test until a human moves it
  forward (the runner never promotes to Done — PDASHOSS01-68).
- `blocked` = real defects found that need a human/dev, **or** the
  test could not be run (missing env / creds / tooling — e.g. the
  UI-kind browser gap of §4.7).
- `paused` = the acceptance criteria are ambiguous ("what does
  'tested' mean for this issue?") — a question for the human.
- `noop` = nothing has changed since the last test pass.

### 4.5 Auto-pause on cap — the TEST arm that must not be forgotten

This is the one site where "just add the registry entry" is
silently wrong, flagged in the create_review_state design review and
re-confirmed at `@main`. `maybe_apply_deferred_pause`
(`scheduling.py:760`) has a hard-coded **REVIEW carve-out** at
`scheduling.py:799`:

```python
if state.group == StateGroup.REVIEW.value:
    logger.info("agent_ticker: review cap hit … leaving it In Review, "
                "no auto-pause")
    return False
```

The intent for In Test (§4.2) is identical to review: on cap
exhaustion the issue must **stay In Test** for a human, not
auto-move to Paused. But once `TEST` is a ticking phase,
`is_ticking_state(state)` at `scheduling.py:789` passes for In Test,
and without a matching carve-out the code falls through to the
non-review path and **auto-pauses** the issue — contradicting the
design. So the carve-out must grow a TEST arm, or (cleaner)
generalize to "any `fresh_session_on_entry` phase stays put on cap
hit" / a `disarm_on_completed`-style flag on `PhaseConfig`.

Recommended: generalize the condition to
`state.group in (StateGroup.REVIEW.value, StateGroup.TEST.value)`,
or better, key it off the registry so future phases inherit the
"stay put on cap, don't auto-pause" behavior by default. Cap-hit
disarm still records `disarm_reason=CAP_HIT`; the difference is only
whether the deferred pause _acts_ on it.

### 4.6 Re-arm on human comment (already symmetric)

The review design made re-arm-on-comment phase-symmetric: any human
comment on an issue whose group is in `CONTINUATION_ELIGIBLE_GROUPS`
re-arms the ticker even after a terminal disarm, honoring
`user_disabled`. Because `CONTINUATION_ELIGIBLE_GROUPS =
tuple(PHASES.keys())`, adding the TEST registry entry makes In Test
inherit this with **zero** code change. "Comment is engagement;
engagement restarts automatic ticking" applies to In Test for free.

### 4.7 Test-kind inference (no new schema)

"In Test" is polymorphic — what the agent should actually do depends
on what the issue produced. v1 enumerates five kinds. This is the
direct answer to the issue's "the test can be different task by
task" worry: don't build a pipeline; infer the cycle at runtime,
exactly as `review` infers CODE / DESIGN / DESIGN_THEN_CODE /
GENERIC.

| Kind                        | Trigger signal                                                              | How the agent tests                                                                                                                                     | Auto-fix?                                                                     |
| --------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `AUTOMATED`                 | impl produced a code artifact (`pr_url` / feature branch in `done_payload`) | check out the branch, run the repo's own gates (`pnpm check`, targeted unit/integration tests, `pnpm build`); add missing tests for the changed surface | trivial fixes → push to PR branch; real defects → `blocked` / follow-up issue |
| `UI` / `EXPLORATORY`        | frontend change whose value is visual/interactive                           | launch the app, drive the changed flow, capture screenshots, check acceptance criteria by observation                                                   | usually no — report; **capability gap, §4.7.1**                               |
| `OPS` / `INFRA`             | config/deploy artifact                                                      | dry-run, validate config, health-check, confirm idempotency                                                                                             | trivial config fixes; else report                                             |
| `DESIGN`                    | a design/doc deliverable (`design_doc_paths`, `.ai_design/` paths)          | verify the deliverable against its acceptance criteria: internal consistency, open questions resolved, testability of what it proposes                  | edit the doc for trivial gaps; else report                                    |
| `NON_TECHNICAL` / `GENERIC` | none of the above                                                           | checklist verification against the stated acceptance criteria; pass/fail assessment per criterion for a human to confirm                                | no — summarize                                                                |

The agent picks the kind at runtime from `parent_done_payload` plus
working-tree inspection (see §5). **No new schema**, no
`Issue.test_kind` field in v1 — same call review made for
`review_kind`. If inference proves unreliable, an explicit
`Issue.test_kind` override (default `auto`) is the v1.5 follow-up,
same `PHASES`-style pluggability as the rest of the system.

The cycle is **uniform across kinds** — this is what makes one
prompt viable:

> derive a test plan from the acceptance criteria → set up the
> environment → execute → collect evidence → validate findings
> (re-run / confirm, no hallucinated failures) → comment results →
> optionally push trivial fixes or file follow-up issues for real
> defects.

Only "how you execute" and "can you auto-fix" change per kind —
exactly how review reuses one find→validate→comment→apply→resolve→
summary loop across its kinds.

#### 4.7.1 Capability gap: UI / e2e

For AUTOMATED / OPS / DESIGN / NON_TECHNICAL, the runner pod already
has what it needs (git, `gh`, the repo toolchain). The gap is UI/e2e:
driving a browser and screenshotting needs a headless browser (e.g.
Playwright) and the ability to boot the app in the pod. **v1 leans on
what the runner can already do** — run the existing suites, type/
lint/build gates, and criterion-checklist verification — and treats
browser-driven UI testing as a follow-up runner capability. When a UI
issue can't be meaningfully tested without a browser, the agent
states that and emits `blocked` (env/tooling missing), which is
honest rather than a false pass.

### 4.8 The lever that decides whether this works: acceptance criteria

The biggest determinant of test quality is **not** the plumbing — a
test is only as good as its acceptance criteria. Two concrete
recommendations:

1. Have the **coding-task** run surface acceptance criteria for the
   test phase to consume. **Correction (third-round review):** there
   is **no** structured artifact-reporting field in `done_payload`
   today. `done_payload` is the harness **run-result envelope** the
   runner posts when a run ends (`runner/views/run_endpoints.py:337`
   → `done_payload=request.data.get("done_payload")`) — it carries
   `conclusion`, the agent's free-text `result`, `status`
   (`completed`/`blocked`/`paused`/`noop`), and usage. Review's own
   kind inference reads that free-text `result` + the working tree,
   **not** a structured `pr_url` / `design_doc_paths` field (grep
   confirms `review-cycle.md` says "look for a `pr_url` in
   done_payload" as a hint, but nothing writes one there). So the
   channel is: have the `coding-task` prompt **state the acceptance
   criteria it validated against in its final summary** (which
   becomes `parent_done_payload.result` for the next phase) and/or in
   a durable issue comment. The `test` prompt then reads
   `parent_done_payload` + the issue/comments as its spec. This is a
   `coding-task` **prompt-body** change (surface the criteria in the
   summary), **not** a new structured `done_payload` field or an M2
   data migration (see §8) — no schema, no harness change.
2. Where criteria are genuinely absent, the test agent derives a
   plan from the description, **states its assumptions in the
   comment**, and tests against them — or `paused`s to ask if the
   deliverable is high-stakes. That keeps the "no criteria → useless
   test" failure mode _visible_ instead of silent.

## 5. Prompt template: `test`

A new global `PromptTemplate` row, `name="test"`, `workspace=NULL`,
`is_active=True`, mirroring the `review` template plumbing that
already ships (`prompting/seed.py:REVIEW_TEMPLATE_BODY` +
`seed_review_template`, `prompting/migrations/0002_review_template.py`).

New recipe (`prompting/recipes.py`): `KIND_TEST = "test"` with a
section list mirroring `KIND_REVIEW`:

```python
KIND_TEST: (
    "test-intro",
    "session-framing",
    "pidash-cli",
    "test-cycle",
    "guardrails",
    "ending-run",
),
```

New sections `prompting/sections/test-intro.md` and
`prompting/sections/test-cycle.md` (analogous to `review-intro.md` /
`review-cycle.md`). The body follows the review template's three-step
shape — **decide kind → run the cycle → emit a done-signal** — with
test-specific kinds. Body sketch (final wording is
prompt-engineering, not design):

```
You are testing the work product of a previous implementation pass.
"Testing" means different things depending on what was produced.

Issue: {{ issue.name }}
Description: {{ issue.description_stripped }}
Recent activity:
{{ comments_section }}
Latest implementation run output (read this carefully — it is your
authoritative record of what was produced, including any acceptance
criteria, pr_url, or design_doc_paths it reported):
{{ parent_done_payload }}

Step 1 — Decide what kind of testing this is.
Inspect parent_done_payload, the issue description, and the working
tree. Choose ONE:
  (a) AUTOMATED — the issue produced code (a PR / feature branch).
      Check out the branch and run the repo's own gates: pnpm check
      (lint + types), the targeted unit/integration tests, pnpm
      build. Add missing tests for the changed surface.
  (b) UI / EXPLORATORY — a frontend change whose value is visual /
      interactive. Launch the app, drive the changed flow, check the
      acceptance criteria by observation. If you cannot boot the app
      or drive a browser in this environment, say so and emit
      `blocked` — do not report a false pass.
  (c) OPS / INFRA — a config / deploy artifact. Dry-run, validate
      config, health-check, confirm idempotency.
  (d) DESIGN — a design / doc deliverable. Verify it against its
      acceptance criteria: internal consistency, open questions
      resolved, testability of what it proposes.
  (e) NON_TECHNICAL / GENERIC — none of the above. Verify the
      deliverable against the stated acceptance criteria, one by
      one, and report a pass/fail assessment for a human to confirm.

If the acceptance criteria are ambiguous or absent, derive a plan
from the description, STATE YOUR ASSUMPTIONS in your comment, and
test against them — or emit `paused` to ask if the deliverable is
high-stakes.

Step 2 — Run the test cycle (uniform across kinds):
  i.   Derive a test plan from the acceptance criteria.
  ii.  Set up the environment for the chosen kind.
  iii. Execute.
  iv.  Collect evidence (test output, coverage, logs, screenshots).
  v.   Validate your findings — re-run / confirm; never report a
       hallucinated failure.
  vi.  Post a STRUCTURED results comment to the pidash issue:
        - Kind detected and what was tested (scope).
        - Method — commands run / flows exercised.
        - Result — pass/fail per acceptance criterion, with evidence.
        - Defects found — and whether you auto-fixed (pushed to the
          PR branch) or it needs a human / a follow-up issue.
  vii. Optionally push trivial fixes to the PR branch, or file a
       follow-up issue for real defects.

Step 3 — Emit a done-signal.
- `completed` = all tests pass / acceptance criteria met.
- `blocked`   = real defects found that need a human/dev, OR the
                test couldn't be run (missing env / creds / tooling).
- `paused`    = acceptance criteria ambiguous — ask the human.
- `noop`      = nothing changed since the last test pass.
```

Seed and migration (mirroring review):

- Add a `TEST_TEMPLATE_BODY` constant + `seed_test_template()` in
  `prompting/seed.py`, analogous to `REVIEW_TEMPLATE_BODY` /
  `seed_review_template`.
- Data migration under `prompting/migrations/` inserting the global
  `test` row if absent (idempotent), modeled on
  `0002_review_template.py`.
- A reseed management command
  (`reseed_test_template.py`) analogous to `reseed_review_template.py`
  for operator prompt iteration.

### 5.1 Results come back as comments — no new plumbing

The runner already authors pidash comments as the agent bot
(`pidash comment add … --as-agent … --agent-run-id …`). Results-as-
comments needs no new machinery — the prompt just instructs the
structured summary comment above each pass (kind, method, per-
criterion result + evidence, defects + disposition).

### 5.2 Only the first run renders the template

As with review, `build_continuation` returns just the new human
comments since the parent run started; the `test` template renders
only on the first (state-entry) run. This is why
`fresh_session_on_entry=True` is load-bearing — the one render that
establishes test intent must be the system prompt of a fresh
session, not a user turn on a resumed review conversation.

## 6. Schema

Minimal — v1 adds **no** new columns.

- **No** `StateGroup` value (TEST exists).
- **No** `State` seed / lifecycle migration (In Test seeded by
  `0154_test_state_group.py`).
- **No** new `Project` cadence fields — v1 reuses
  `agent_review_default_*` (§3.2).
- **No** new `IssueAgentTicker` override fields — v1 reuses
  `review_interval_seconds` / `review_max_ticks`.

The only "schema" change is the `test` PromptTemplate row inserted by
the prompting data migration (§5). Everything else is code:
the registry entry, the third arm in the cadence resolvers, and the
TEST carve-out in `maybe_apply_deferred_pause`.

If field experience shows test rhythm must diverge from review, the
follow-up adds `Project.agent_test_default_interval_seconds` /
`agent_test_default_max_ticks` + `IssueAgentTicker.test_interval_seconds`
/ `test_max_ticks`, and the cadence resolvers gain a genuine third
pair instead of aliasing review's. Structuring §3.1's resolvers as a
phase-keyed lookup now makes that follow-up a data change rather than
a control-flow change.

## 7. Code touchpoints

Concentrated. The registry entry carries the trigger/wake/template
policy; the rest is the third arm at each binary cadence/pause site
plus the new prompt.

### 7.1 `orchestration/agent_phases.py`

Add the `StateGroup.TEST.value: PhaseConfig("In Test", "test",
fresh_session_on_entry=True)` entry to `PHASES`. This alone lights up
`_is_delegation_trigger`, `CONTINUATION_ELIGIBLE_GROUPS`,
`fire_tick`, `template_name_for`, the leaving-a-ticking-group disarm,
and re-arm-on-comment.

### 7.2 `db/models/issue_agent_ticker.py`

`_is_review_phase()` + the two `effective_*()` resolvers
(`:131` / `:150` / `:175`) branch binary review-vs-impl. Add a test
arm: when the issue's current state is the In Test phase, resolve the
test cadence pair (v1: the review pair). Cleanest form is a small
`_phase_cadence_fields(state) → (override_attr, project_default_attr)`
helper keyed on `state.group` that all three methods share; v1 may
instead add an explicit `elif` if it reads more clearly against the
existing review code.

### 7.3 `orchestration/scheduling.py`

- `_project_default_interval` (`:64`) / `_project_default_max_ticks`
  (`:85`): add a TEST arm returning the test-phase default (v1: the
  `agent_review_default_*` value), or generalize to the phase-keyed
  lookup.
- `maybe_apply_deferred_pause` (`:760`, carve-out at `:799`): add the
  TEST arm so a cap-exhausted In Test issue **stays In Test** and is
  not auto-paused (§4.5). Prefer generalizing the condition to a
  registry-keyed "stay put on cap" check.

### 7.4 `orchestration/service.py`

- `CONTINUATION_ELIGIBLE_GROUPS` (`:304`) is already
  `tuple(PHASES.keys())` — no change; it grows automatically.
- `_is_delegation_trigger` (`:109`) is already
  `is_ticking_state(to_state)` — no change.
- `handle_issue_state_transition` (`:154-224`) cross-phase resume:
  **no production-code change needed** (verified at `@main` — §4.3).
  The existing capture gate at `service.py:156`
  (`from_state.group == StateGroup.STARTED.value`) fires only on
  leaving In Progress, and the sole `resume_parent_run` write
  (`service.py:174`) is gated by it, so a review→test pass-through
  leaves the captured impl run intact; `arm_ticker` / `disarm_ticker`
  don't touch the field either. A later kick-back to In Progress
  therefore still resumes the implementation thread via the
  `fresh_session_on_entry == False` branch (`:211-214`). **Do not**
  add "explicit capture" code here — it would risk introducing the
  clobber it aims to prevent. The only work is a regression test
  (§9) proving the capture survives the impl→review→test→In-Progress
  chain.

### 7.5 `bgtasks/agent_ticker.py`

`scan_due_tickers` `effective_cap` annotation (`:58`) is a
`Case(When(group==REVIEW …))`. Add a `When(group==TEST …)` arm (v1:
same `agent_review_default_max_ticks` pair), or refactor to a
phase-keyed annotation. `fire_tick` (`:132`) needs no change —
already `is_ticking_state`-gated.

### 7.6 `prompting/recipes.py`

Add `KIND_TEST = "test"` and its `RECIPES` entry (§5). Confirm
`kind_for` maps the `test` template name to `KIND_TEST`.

### 7.7 `prompting/seed.py` + `prompting/sections/` + migration

- Add `TEST_TEMPLATE_BODY` + `seed_test_template()` in `seed.py`.
- Add `prompting/sections/test-intro.md` and
  `prompting/sections/test-cycle.md`.
- Data migration inserting the global `test` template row
  (idempotent), modeled on `0002_review_template.py`.
- `reseed_test_template.py` management command.
- **Load-bearing:** extend the `coding-task` template body so impl
  runs **state the acceptance criteria they validated against in the
  final run summary** (which the next phase reads as
  `parent_done_payload.result`) and/or a durable comment — the test
  agent's spec (§4.8). This is a prompt-body change only; there is
  **no** structured `done_payload` artifact field to extend (§4.8
  correction), so no schema and no harness change.

### 7.8 `prompting/sections/state-routing.md`

Rewrite the `test` branch (`:14`) — it currently tells the agent
"Automatic ticking is not wired to this group, so you should not
normally be invoked here." Once the phase is live, describe the test
cycle and that In Test now ticks.

## 8. Migrations

Two, both under `prompting/migrations/` — no `db/migrations/` change
in v1 (no schema columns added):

**M1 — `test` prompt template.** `RunPython` inserting the global
`PromptTemplate(name="test", workspace=NULL, is_active=True,
body=TEST_TEMPLATE_BODY)` if absent. Idempotent, reversible. Modeled
on `0002_review_template.py`.

**M2 — `coding-task` body update.** `RunPython` extending the
existing `coding-task` template body so the impl run **states the
acceptance criteria it validated against in its final summary**
(→ `parent_done_payload.result`) and/or a durable comment (§4.8).
This is a prompt-body edit — **not** a new structured `done_payload`
field (§4.8 correction). Idempotent — only updates if the body lacks
the instruction. Reversible.

(If the impl summary already surfaces acceptance criteria clearly,
M2 collapses into a no-op / is dropped — verify during impl.)

## 9. Tests

- Extend `test_agent_phases.py`: registry now has three entries;
  `is_ticking_state` / `phase_config_for` / `template_name_for` true
  for In Test, exhaustive over every `StateGroup`.
- Extend `test_scheduling.py`:
  - `arm_ticker` on In Test uses the **test-phase** effective
    interval/max (v1: the review default) and clears `disarm_reason`.
  - `maybe_apply_deferred_pause` **does not** auto-pause a
    cap-exhausted In Test issue — it stays In Test (the §4.5 arm).
    Regression-guard: In Progress cap-hit still auto-pauses.
  - `_project_default_interval` / `_project_default_max_ticks` return
    the test-phase default for an In Test issue.
- Extend `test_issue_agent_ticker.py`: `effective_interval_seconds()`
  / `effective_max_ticks()` resolve the test pair for In Test across
  the override / default permutations.
- Extend `test_agent_ticker.py`: `fire_tick` ticks an In Test issue;
  `scan_due_tickers` admits an In Test row below its cap and skips it
  at cap; non-ticking states stay skipped.
- Extend `test_service.py`:
  - `_is_delegation_trigger` true for In Test.
  - In Review → In Test dispatches a **fresh session**
    (`parent_run=None`, `pinned_runner_id` cleared).
  - **Regression guard (§4.3):** the captured implementation
    `resume_parent_run` survives the impl→review→test path with the
    _existing_ code unchanged, so a kick-back to In Progress resumes
    the implementation thread. This test is the entire cost of the
    3-phase resume concern — no production-code change.
  - Comment on In Test wakes the agent
    (`CONTINUATION_ELIGIBLE_GROUPS` includes `test`).
- Extend `prompting/test_composer.py`: `build_first_turn` selects the
  `test` template for In Test, `review` for In Review, `coding-task`
  for In Progress; fallback for any other group.
- Migration tests: M1 inserts the `test` template idempotently; M2
  updates the `coding-task` body idempotently (or is dropped if the
  criteria field already ships).

## 10. UI

Minimal v1 surface. The **In Test** column already renders — the
review work updated the hard-coded state-group enumerations
(`packages/constants/src/state.ts`,
`apps/api/pi_dash/space/utils/grouper.py`, `api/views/issue.py`,
`utils/order_queryset.py`) when it added the `review` group; confirm
the `test` group is present in each (it should be, since In Test was
seeded — verify during impl, add any missing arm). The issue-detail
"next agent check" row already reads `next_run_at` from the ticker —
no change. Cap-hit copy on an In Test issue reuses the review copy in
v1.

## 11. Open questions

| #   | Question                                                         | Decision                                                                                                                                                                                                                                                                                                                                              |
| --- | ---------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Q1  | Native session resume across phase change into test?             | **No — fresh session on cross-group entry.** §4.3. The `test` system prompt must be the system prompt, not a user turn on a resumed review session.                                                                                                                                                                                                   |
| Q2  | Separate cadence for test vs review?                             | **No (v1) — reuse the review defaults (3 h × 4).** §3.2. Dedicated `agent_test_default_*` is a one-migration follow-up if rhythm needs to diverge.                                                                                                                                                                                                    |
| Q3  | Auto hand-off In Review → In Test via a review done-signal?      | **No (v1)** — manual user transition only. Same call review made for impl→review.                                                                                                                                                                                                                                                                     |
| Q4  | Custom workspace state names in the `test` group?                | **Don't tick (v1)** — same restriction as In Progress / In Review.                                                                                                                                                                                                                                                                                    |
| Q5  | May the test agent write code / edit artifacts, or only comment? | **Allowed to push trivial fixes and file follow-up issues (v1).** §4.7. AUTOMATED kind may push trivial fixes to the PR branch; real defects → `blocked` / follow-up. DESIGN may edit the doc for trivial gaps. NON_TECHNICAL only summarizes. The validate-findings step (re-run before acting) is the hallucination guard.                          |
| Q6  | UI / e2e testing in the runner pod?                              | **Follow-up capability, not a v1 blocker.** §4.7.1. v1 leans on the existing toolchain; a UI issue that needs a browser the pod lacks → honest `blocked`.                                                                                                                                                                                             |
| Q7  | Explicit `Issue.test_kind` override?                             | **No (v1) — runtime inference.** §4.7. An `Issue.test_kind` (default `auto`) is the v1.5 follow-up if inference proves unreliable, same escape hatch review reserved for `review_kind`.                                                                                                                                                               |
| Q8  | Where do the test acceptance criteria come from?                 | **From the impl run's final summary (`parent_done_payload.result`) + issue comments** — there is no structured `done_payload` artifact field (§4.8 correction, third-round review). Extend the `coding-task` prompt to state the criteria it validated against in its summary; absent that, the test agent derives + states assumptions or `paused`s. |
| Q9  | Cap-hit behavior for In Test?                                    | **Stay In Test, no auto-pause** — §4.5. Requires the TEST arm in `maybe_apply_deferred_pause`; without it a cap-exhausted issue is silently moved to Paused.                                                                                                                                                                                          |

## 12. PR sequence

A single PR is appropriate — In Test reuses the review scaffold
wholesale and adds no schema columns. Shipping the state activation
without the prompt would render an implementation/review prompt
against a test-named state, so they must land together.

**PR — Activate the In Test phase + `test` prompt**

- Add the `StateGroup.TEST` entry to `agent_phases.PHASES`
  (`fresh_session_on_entry=True`).
- Add the TEST arm to the cadence resolvers
  (`issue_agent_ticker.effective_*`, `scheduling._project_default_*`,
  `agent_ticker.scan_due_tickers` annotation) — reusing the review
  cadence pair in v1. Prefer a phase-keyed lookup so a future
  dedicated test cadence is a data change.
- Add the TEST arm to `maybe_apply_deferred_pause`'s "stay put on
  cap" carve-out (§4.5) — the one silently-wrong-without-it site.
- Add a **regression test** proving the existing `resume_parent_run`
  capture survives the 3-phase impl→review→test chain (§4.3 / §7.4).
  No production-code change — the existing capture gate already
  handles it (verified at `@main`).
- Add `KIND_TEST` + recipe, `test-intro` / `test-cycle` sections,
  `TEST_TEMPLATE_BODY` + `seed_test_template`, the insert migration,
  and the reseed command.
- Extend the `coding-task` body to report acceptance criteria in
  `done_payload` (§4.8), if review's PR didn't already.
- Rewrite `state-routing.md`'s `test` branch.
- Tests per §9.

**Follow-ups** (each independently shippable):

- Browser / e2e capability in the runner (§4.7.1).
- Dedicated `Project.agent_test_default_*` + `IssueAgentTicker.test_*`
  cadence pair, if test rhythm must diverge from review (§6).
- Explicit `Issue.test_kind` override if runtime inference proves
  unreliable (§4.7 / Q7).
- Auto In Review → In Test hand-off on a review done-signal (Q3).

---

## Appendix A — Worked timeline

```
T=0      Issue I in In Review (a review run R_rev already emitted
         `completed` — approved; ticker disarmed, issue sitting).
T+5m     User moves I → In Test (group=review → group=test).
         • "leaving a ticking group disarms" fires (transient).
         • is_ticking_state(In Test) ✓ via the new PHASES entry →
           arm_ticker on the test-phase effective cadence
           (review default in v1: 3 h × 4). disarm_reason cleared.
         • fresh_session_on_entry=True → R_test1 dispatched with
           parent_run=None, pinned_runner_id cleared. The captured
           ticker.resume_parent_run still points at the pre-review
           implementation run R_impl (NOT R_rev) — preserved across
           the pass-through.
         • R_test1's first turn renders the `test` template as the
           system prompt of a fresh session. Step 1 inspects
           R_impl's done_payload: pr_url present → AUTOMATED kind.
T+40m    R_test1 checks out the PR branch, runs `pnpm check` + the
         targeted suites + `pnpm build`, finds one failing unit test
         it can fix trivially, pushes the fix to the PR branch, and
         posts a structured results comment (kind=AUTOMATED, method,
         per-criterion pass/fail, the fix pushed). Emits `completed`.
         Consumer terminate: maybe_disarm_on_terminal_signal sees
         `completed` → ticker disarmed. Issue stays In Test.
T+3h     No new human comment; ticker is disarmed → no tick fires.
T+later  Human reads the results comment, comments "please also
         cover the empty-input edge case." Comment on an In Test
         issue → re-arm-on-comment (CONTINUATION_ELIGIBLE_GROUPS
         includes `test`) flips enabled=True, and a continuation run
         R_test2 dispatches — native resume on R_test1's thread, the
         `test` system prompt persists, build_continuation feeds the
         new human comment. R_test2 adds the edge-case test.
...
T+~12h   If the human never engaged and ticking had continued, the
         cap (4 ticks) would be hit. enabled=false; on terminate,
         maybe_apply_deferred_pause sees is_ticking_state(In Test) ✓
         but the NEW test carve-out (§4.5) returns False → issue
         STAYS In Test (no auto-pause), waiting for a human. Without
         that arm it would have been silently moved to Paused.
```
