---
key: state-routing
title: State routing
customizable: locked
---
## State routing (Step 0)

Based on `issue.state_group`:

- `backlog` — do nothing. The issue is not ready for work. Post one short comment and exit:
  `pidash comment add {{ issue.identifier }} --body "Delegation received while in backlog; no action taken."`
- `unstarted` — this should normally not happen, because orchestration should create runs only after delegation. If you are invoked on an `unstarted` issue anyway, record that mismatch in the workpad `Notes` section and proceed cautiously. Do not move the issue to a completed state solely because of the mismatch.
- `started` — this is active execution. Proceed through Step 0.5 (analyze & scope), then Step 1 and Step 2 below. Do not skip the analyze step; cutting a branch before deciding `proceed` / `clarify` / `split` is a defect.
- `test` — the issue is in the **In Test** state (group `test`), a first-class ticking phase like In Progress and In Review. Moving an issue here wakes an agent on the same tick infrastructure, rendering the polymorphic `test` prompt: infer the test kind (AUTOMATED / UI / OPS / DESIGN / NON_TECHNICAL) from what the issue produced, run that test cycle, and post a structured results comment. A `completed` pass leaves the issue In Test for a human to move forward; real defects or an un-runnable test are `blocked`; ambiguous criteria are `paused`. See the "Test cycle" section for the full flow. When you are working an issue in another state and its task calls for a testing/QA phase, `In Test` is the state to move it to.
- `completed` — the issue is already done. Post a short noop comment and exit without moving state.
- `cancelled` — the issue was cancelled. Post a short noop comment and exit without moving state.
