---
key: software.test
title: Software testing guidance
customizable: overridable
---

## Testing software deliverables (work type: software)

### Sync the PR branch with its base before testing

Before you test anything, bring the {{ repo.code_review_term }} branch up to date with its base. Every test verdict must describe code that can actually merge into today's base — test only the synced branch, never a stale one.

- `git fetch origin`, then check out the PR's head branch. The base is the PR's base branch; when that is not visible, fall back to `{{ repo.base_branch }}` (or the repository's default branch when no base is set). For a stacked PR — the base is another PR's branch, not the project base — sync against that same base branch.
- `git merge origin/<base>` — merge, never rebase, and never force-push. The PR branch is shared and may already carry review history; a merge commit preserves both sides.
- Clean merge: run this repo's own gates on the merged tree (README/CONTRIBUTING, `.github/workflows/`, package manifests — discover them, never assume a toolchain). Push the merge commit if it is not a no-op.
- Conflicts: resolve them so the base's newer structure wins and the PR's change is re-applied on top. Run this repo's own gates, then push the merge commit.
- Too large to resolve with confidence — the base rewrote the code the PR changes: do not test stale code. Abort the merge, leave the branch untouched, comment on the issue with what changed on the base and what needs redoing, and move the issue back to **In Progress** so the change can be rebuilt on the current base.
- Record in the workpad the base SHA you were checked against (`git rev-parse origin/<base>`, or the short SHA after the sync), so a later run can tell whether the verdict is still current. For a stacked PR, note in the workpad whether the parent PR has merged.
- If a prior test verdict exists and the base has moved since that verdict, redo the test on the synced branch — a verdict on an old base does not carry over. A merge into the base after the test ran means syncing and re-checking first.

Gates and CI runs are **corroborating evidence, never the verdict**. A green pipeline only re-checks what someone already wrote a check for — it cannot tell you the new behavior is right (its tests are part of the change being doubted) or that the button is actually visible. A test pass that ends with "CI is green" and no first-hand observation is incomplete.

Software deliverables come in these test kinds — prefer them over NON_TECHNICAL / GENERIC when they match; each is a way of impersonating the change's user:

- **AUTOMATED** — the user is **a program**: an API client, CLI invocation, library caller, or another service (the issue produced code — look for a `pr_url` in the payload or a feature branch ahead of the base branch). Check out the branch and act as that consumer: make real calls with real payloads against a running instance where feasible, and run **this repo's own gates** as corroboration — discover them, never assume a toolchain. Read the README/CONTRIBUTING, the CI config (`.github/workflows/`, `.gitlab-ci.yml`), and the package manifest (`package.json` scripts, `Makefile`, `pyproject.toml`, `Cargo.toml`, `go.mod`, `build.gradle`) and run what they declare for format/lint, types, tests, and build. In a polyglot repo run the gates for the stack you actually changed. Run the targeted unit/integration tests for the changed surface, and add missing tests where it's cheap and clearly in scope. If you cannot determine the gates, say so in your results comment rather than inventing commands.
- **UI / EXPLORATORY** — the user is **a human at a screen**: a frontend change whose value is visual / interactive. Launch the app, drive the changed flow _as that human would_, check the acceptance criteria by observation, and click through the adjacent flows the change could plausibly have disturbed. Unit tests and a clean build do **not** substitute for looking at it. **If you cannot boot the app or drive a browser in this environment, say so plainly and emit `blocked` (missing tooling) — do not report a false pass.**
- **OPS / INFRA** — the user is **an operator** (or the deploy machinery itself): a config / deploy artifact. Run the procedure the operator would run: dry-run, validate the config, apply where safe, health-check the result, confirm idempotency. A config that merely parses is not tested. Note that ops changes often produce **no CI signal at all** — your first-hand run may be the only verification this change gets.
- **DESIGN** — the user is **a reader who must act on the doc** (e.g. paths under `.ai_design/`). Read it cold, as someone who wasn't in the conversation: internal consistency, open questions resolved, and whether what it proposes is actually implementable/testable from the text alone.

To act as the user you need **somewhere the software runs**. Obtain it by the cheapest workable means: run/boot it locally; stand up an ephemeral environment (containers, seed data) if it needs setup; or use the project's own pipeline (a preview deploy, a CI workflow that produces a running instance) when local cannot work end-to-end. The pipeline is a way to _get_ an environment — its exit code is a data point, not the verdict. If no route yields a place to act as the user, that is an honest `blocked` (say exactly what was missing), not a downgraded pass.

Acting on findings, within the limits of the chosen kind (step 7 of the test cycle):

- **AUTOMATED**: push a _trivial_ fix to the {{ repo.code_review_term }} branch (re-run the gates first to confirm it's green); for a real defect, emit `blocked` and/or file a follow-up issue rather than hand-waving a pass.
- **UI / EXPLORATORY**: same rule as AUTOMATED for a trivial fix; a visual/interaction defect you cannot fix trivially goes back as a defect finding.
- **OPS / INFRA**: apply a trivial config fix; otherwise report.
- **DESIGN**: edit the doc for a trivial gap; otherwise report.

For software, "waiting on an external process" means waiting on CI checks or a merge.
