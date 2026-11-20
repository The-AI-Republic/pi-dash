---
key: software.test
title: Testing software
customizable: overridable
---
### Software

Test the synced branch, never a stale one: `git fetch origin`, check out the {{ repo.code_review_term }} branch, `git merge origin/<base>` (the {{ repo.code_review_term }}'s base, else `{{ repo.base_branch or "the default branch" }}`; never rebase, never force-push), run the repository's own checks, and push the merge if it is not a no-op. If the base rewrote the code under test and you cannot resolve it with confidence, abort the merge and send the issue back to In Progress saying what changed. If the base has moved since an earlier verdict, test again.

The repository's own checks are discovered, never assumed: read the README/CONTRIBUTING, the CI config (`.github/workflows/`, `.gitlab-ci.yml`) and the package manifests (`package.json` scripts, `Makefile`, `pyproject.toml`, `Cargo.toml`, `go.mod`), and run what they declare for the stack you touched. If you cannot determine them, say so instead of inventing commands. Green CI is supporting evidence, not a verdict.

Act as the consumer. A program (API client, CLI, library caller): make real calls with real inputs against a running instance. A person at a screen: launch the app and drive the changed flow; if you cannot boot it or drive a browser here, finish `blocked` rather than pass. An operator: run the procedure, dry-run first, health-check the result. A reader of a design document: read it cold and check it can be acted on without its author.

To act as the user you need somewhere the software runs: run it locally, stand up an ephemeral environment (containers, seed data), or use a preview deploy the project's own automation produces.

Fixing: push only a trivial fix to the branch (re-run the checks first); anything else goes back as a defect. Waiting on CI or on a merge is `waiting_on_external`.
