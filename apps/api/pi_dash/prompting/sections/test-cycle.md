---
key: test-cycle
title: Test cycle
customizable: overridable
---

## Step 0 — Gather the acceptance criteria (the spec you test against)

A test is only as good as the criteria it checks against. Before deciding
anything, assemble the spec from every channel available to you:

- the issue description and any `Validation` / `Test Plan` / `Testing`
  section in it,
- the recent comment activity (humans often refine the criteria there),
- the implementation run's summary in `parent_done_payload` (it should
  state what it built and validated against),
- the issue **workpad** — read it with `pidash workpad get` for the
  `Acceptance Criteria` and `Validation` sections the implementation run
  recorded.

If the criteria are genuinely absent or ambiguous, derive a plan from the
description, **state your assumptions explicitly in your results comment**,
and test against them — or, if the deliverable is high-stakes, emit
`paused` and ask the human what "tested" should mean here.

## Step 1 — Decide what kind of testing this is

Inspect `parent_done_payload`, the issue description, and the working tree.
Choose ONE:

- **(a) AUTOMATED** — the issue produced code (a PR / feature branch;
  look for a `pr_url` in the payload or a feature branch ahead of the base
  branch). Check out the branch and run the repo's own gates: `pnpm check`
  (lint + types), the targeted unit/integration tests for the changed
  surface, `pnpm build`. Add missing tests for the changed surface where
  it's cheap and clearly in scope.
- **(b) UI / EXPLORATORY** — a frontend change whose value is visual /
  interactive. Launch the app, drive the changed flow, and check the
  acceptance criteria by observation. **If you cannot boot the app or
  drive a browser in this environment, say so plainly and emit `blocked`
  (missing tooling) — do not report a false pass.**
- **(c) OPS / INFRA** — a config / deploy artifact. Dry-run, validate the
  config, health-check, confirm idempotency.
- **(d) DESIGN** — a design / doc deliverable (e.g. paths under
  `.ai_design/`). Verify it against its acceptance criteria: internal
  consistency, open questions resolved, and testability of what it
  proposes.
- **(e) NON_TECHNICAL / GENERIC** — none of the above. Verify the
  deliverable against the stated acceptance criteria one by one, and
  report a pass/fail assessment per criterion for a human to confirm.

## Step 2 — Run the test cycle (uniform across kinds)

1. Derive a concrete test plan from the acceptance criteria.
2. Set up the environment for the chosen kind.
3. Execute the plan.
4. Collect evidence — test output, coverage, logs, screenshots (as links).
5. Validate your findings — re-run / confirm before you report anything;
   never report a hallucinated failure. Drop any finding you can't
   reproduce.
6. Post a **structured results comment** to the pidash issue:
   - **Kind** detected and **what was tested** (scope).
   - **Method** — the commands you ran / the flows you exercised.
   - **Result** — pass/fail *per acceptance criterion*, with evidence.
   - **Defects found** — and, for each, whether you auto-fixed it (pushed
     to the PR branch) or it needs a human / a follow-up issue.
7. Optionally act on the findings, within the limits of the chosen kind:
   - **AUTOMATED**: push a *trivial* fix to the PR branch (re-run the
     gates first to confirm it's green); for a real defect, emit `blocked`
     and/or file a follow-up issue rather than hand-waving a pass.
   - **DESIGN**: edit the doc for a trivial gap; otherwise report.
   - **OPS**: apply a trivial config fix; otherwise report.
   - **NON_TECHNICAL**: summarize only — do not mutate the deliverable.

## Step 3 — Emit a done-signal

The test pass concludes by matching the outcome (see "Available states"
and "Ending the run"):

- **completed** — all tests pass / every acceptance criterion is met.
  Post your results comment and **leave the issue In Test**. Do **not**
  move it to a `completed`/Done state: the runner never promotes an issue
  to Done — a human closes it once they've seen the results, or a separate
  supporting process does. (Test ticking is bounded; once it's exhausted
  the issue simply stays In Test. If a later tick finds nothing has
  changed, emit a **noop** and exit without moving state.)
- **blocked** — real defects that need a human/dev, **or** the test could
  not be run (missing env / creds / tooling — e.g. no browser for a UI
  kind). Follow "Blocking the run" (post the results comment, move to
  "Blocked" if the project has that state).
- **paused** — the acceptance criteria are ambiguous or absent and the
  deliverable is high-stakes: a clarifying question for the human. Follow
  "Blocking the run".
- **noop** — nothing has changed since your last test pass. Post a short
  noop comment and exit without moving state.
