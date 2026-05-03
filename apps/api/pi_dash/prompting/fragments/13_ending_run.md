## Ending the run

There is no fenced `pi-dash-done` block. The cloud does not parse your final turn message. A run ends when the agent process exits; Pi Dash learns the outcome by looking at the issue itself. Before your final turn ends you must have already done all of the following via `pidash`:

### Workpad completeness — verify before exit (always, regardless of outcome)

The workpad comment is the only carrier of state into the next run. The next agent will start from a fresh session with no memory of this one — anything you do not write here is lost. Before exit, confirm each of the following on the workpad:

- [ ] `### Phase` reflects the current state (not stale from a prior run).
- [ ] `### Progress Checkpoints` match what is actually true in the repo and the issue right now. Items that don't apply to this task are marked `n/a` (e.g. `- [x] pr_opened (n/a)`), not left unchecked.
- [ ] `### Analysis` is populated: `Restated problem`, `Acceptance criteria`, `Proposed approach`, `Task type`, `Risks / assumptions`, `Decision`. No placeholder text.
- [ ] `### Plan` reflects the current plan, with checked-off items reflecting current reality. New scope discovered this run is added; obsolete items are removed or marked done.
- [ ] `### Notes` captures anything material learned this run that the next run needs to know — non-obvious decisions, dead ends ruled out, environment quirks, file/symbol pointers used.
- [ ] If exiting in a blocked state (see §2 below): `### Autonomy / Escalation` has `safe_to_continue: false`, `Reason:` explains why, and `Question for human:` is a specific actionable question (not `null`, not vague).

If any item fails this check, fix the workpad before exiting. A workpad missing required fields means the next tick has to redo investigation or re-ask the human — that is the failure mode this discipline prevents.


1. **If the work completed successfully:**
   - Pushed your branch (if any code changed).
   - Updated the workpad comment so its checkpoints, validation notes, and top-of-pad summary are accurate.
   - Moved the issue to a state in the `completed` group (usually "Done"):
     `pidash issue patch {{ issue.identifier }} --state "Done"`
     Pick the state from "Available states" whose `group` is `completed` and whose name best matches the workflow.

2. **If the run is blocked** (missing auth, missing access, or a decision only a human can make):
   - Recorded the blocker in the workpad, including the specific human action needed.
   - Posted a dedicated comment prefixed `Blocked:` summarizing what is blocking and what is needed:
     `pidash comment add {{ issue.identifier }} --body-file ./.pidash-blocked.md`
     (Any path under the working directory works; avoid `/tmp/` since sandbox policy may block it.)
   - If the project has a state whose name is "Blocked" (see "Available states"), move the issue there:
     `pidash issue patch {{ issue.identifier }} --state "Blocked"`
     If no "Blocked" state exists, leave the issue in its current state. The `Blocked:` comment is the signal.

3. **If the issue was already terminal or not workable** (you were invoked on a `completed` / `cancelled` / `backlog` issue, or there is genuinely nothing to do):
   - Post a single short comment explaining what you observed.
   - Do not move state.

The authoritative record of this run lives on the issue: its state, the workpad comment, and any comments you posted. The runner reports only process `exit_code` and elapsed seconds to the cloud.
