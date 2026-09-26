---
key: software.test
title: Software testing guidance
customizable: overridable
---

## Testing software deliverables (work type: software)

Gates and CI runs are **corroborating evidence, never the verdict**. A green pipeline only re-checks what someone already wrote a check for — it cannot tell you the new behavior is right (its tests are part of the change being doubted) or that the button is actually visible. A test pass that ends with "CI is green" and no first-hand observation is incomplete.

Software deliverables come in these test kinds — prefer them over NON_TECHNICAL / GENERIC when they match; each is a way of impersonating the change's user:

- **AUTOMATED** — the user is **a program**: an API client, CLI invocation, library caller, or another service (the issue produced code — look for a `pr_url` in the payload or a feature branch ahead of the base branch). Check out the branch and act as that consumer: make real calls with real payloads against a running instance where feasible, and run **this repo's own gates** as corroboration — discover them, never assume a toolchain. Read the README/CONTRIBUTING, the CI config (`.github/workflows/`, `.gitlab-ci.yml`), and the package manifest (`package.json` scripts, `Makefile`, `pyproject.toml`, `Cargo.toml`, `go.mod`, `build.gradle`) and run what they declare for format/lint, types, tests, and build. In a polyglot repo run the gates for the stack you actually changed. Run the targeted unit/integration tests for the changed surface, and add missing tests where it's cheap and clearly in scope. If you cannot determine the gates, say so in your results comment rather than inventing commands.
- **UI / EXPLORATORY** — the user is **a human at a screen**: a frontend change whose value is visual / interactive. Launch the app, drive the changed flow _as that human would_, check the acceptance criteria by observation, and click through the adjacent flows the change could plausibly have disturbed. Unit tests and a clean build do **not** substitute for looking at it. **If you cannot boot the app or drive a browser in this environment, say so plainly and emit `blocked` (missing tooling) — do not report a false pass.**
- **OPS / INFRA** — the user is **an operator** (or the deploy machinery itself): a config / deploy artifact. Run the procedure the operator would run: dry-run, validate the config, apply where safe, health-check the result, confirm idempotency. A config that merely parses is not tested. Note that ops changes often produce **no CI signal at all** — your first-hand run may be the only verification this change gets.
- **DESIGN** — the user is **a reader who must act on the doc** (e.g. paths under `.ai_design/`). Read it cold, as someone who wasn't in the conversation: internal consistency, open questions resolved, and whether what it proposes is actually implementable/testable from the text alone.

To act as the user you need **somewhere the software runs**. Obtain it by the cheapest workable means: run/boot it locally; stand up an ephemeral environment (containers, seed data) if it needs setup; or use the project's own pipeline (a preview deploy, a CI workflow that produces a running instance) when local cannot work end-to-end. The pipeline is a way to *get* an environment — its exit code is a data point, not the verdict. If no route yields a place to act as the user, that is an honest `blocked` (say exactly what was missing), not a downgraded pass.

Acting on findings, within the limits of the chosen kind (step 7 of the test cycle):

- **AUTOMATED**: push a *trivial* fix to the {{ repo.code_review_term }} branch (re-run the gates first to confirm it's green); for a real defect, emit `blocked` and/or file a follow-up issue rather than hand-waving a pass.
- **UI / EXPLORATORY**: same rule as AUTOMATED for a trivial fix; a visual/interaction defect you cannot fix trivially goes back as a defect finding.
- **OPS / INFRA**: apply a trivial config fix; otherwise report.
- **DESIGN**: edit the doc for a trivial gap; otherwise report.
