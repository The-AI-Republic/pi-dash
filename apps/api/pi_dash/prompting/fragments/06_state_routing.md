## State routing (Step 0)

Based on `issue.state_group`:

- `backlog` — do nothing. The issue is not ready for work. Post one short comment and exit:
  `pidash comment add {{ issue.identifier }} --body "Delegation received while in backlog; no action taken."`
- `unstarted` — this should normally not happen, because orchestration should create runs only after delegation. If you are invoked on an `unstarted` issue anyway, record that mismatch in the workpad `Notes` section and proceed cautiously. Do not move the issue to a completed state solely because of the mismatch.
- `started` — this is active execution. Proceed through Step 1 and Step 2 below.
- `completed` — the issue is already done. Post a short noop comment and exit without moving state.
- `cancelled` — the issue was cancelled. Post a short noop comment and exit without moving state.
