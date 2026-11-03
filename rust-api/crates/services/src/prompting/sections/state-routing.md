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
- `started` / `review` / `test` — the issue is in the ticking bucket (In Progress / In Review / In Test). See "Task lifecycle" for the path, the exit conditions, and the allowed moves. For `started` this is active execution: proceed through Step 0.5 (analyze & scope), then Step 1 and Step 2 below. Do not skip the analyze step; cutting a branch before deciding `proceed` / `clarify` / `split` is a defect. When you are working an issue in another state and its task calls for a testing/QA phase, `In Test` is the state to move it to.
- `completed` — the issue is already done. Post a short noop comment and exit without moving state.
- `cancelled` — the issue was cancelled. Post a short noop comment and exit without moving state.
