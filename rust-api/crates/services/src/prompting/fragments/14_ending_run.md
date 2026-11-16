## Ending the run

There is no fenced `pi-dash-done` block. The cloud does not parse your final turn message. A run ends when the agent process exits; Pi Dash learns the outcome by looking at the issue itself. Before your final turn ends you must have already done all of the following via `pidash`:

### Workpad completeness — verify before exit (always, regardless of outcome)

The workpad is the only carrier of state into the next run. The next agent will start from a fresh session with no memory of this one — anything you do not write here is lost. Before exit, confirm each of the following on the workpad (the body you `pidash workpad update`'d most recently):

- [ ] `### Phase` reflects the current state (not stale from a prior run).
- [ ] `### Progress Checkpoints` match what is actually true in the repo and the issue right now. Items that don't apply to this task are marked `n/a` (e.g. `- [x] pr_opened (n/a)`), not left unchecked.
- [ ] `### Analysis` is populated: `Restated problem`, `Acceptance criteria`, `Proposed approach`, `Task type`, `Risks / assumptions`, `Decision`. No placeholder text.
- [ ] `### Plan` reflects the current plan, with checked-off items reflecting current reality. New scope discovered this run is added; obsolete items are removed or marked done.
- [ ] `### Notes` captures anything material learned this run that the next run needs to know — non-obvious decisions, dead ends ruled out, environment quirks, file/symbol pointers used.
- [ ] If exiting in a blocked state (see §2 below): `### Autonomy / Escalation` has `safe_to_continue: false`, `Reason:` explains why, and `Awaiting human reply:` carries a one-line note (date + gist) pointing at the comment you posted. This is a self-note for the next run — no code reads it; the run that picks up after the human replies should still confirm by reading the recent comments.

If any item fails this check, fix the workpad before exiting — `pidash workpad update --body-file <path>`. A workpad missing required fields means the next tick has to redo investigation or re-ask the human, which is the failure mode this discipline prevents.


1. **If the work completed successfully:**
   - Pushed your branch (if any code changed).
   - Wrote the final workpad via `pidash workpad update` so its checkpoints and validation notes are accurate.
   - Optionally posted a short completion comment (e.g. "Done — PR <url> merged-ready") if the human will benefit from the ping; otherwise the issue state change is signal enough.
   - Moved the issue to a state in the `completed` group (usually "Done"):
     `pidash issue patch {{ issue.identifier }} --state "Done"`
     Pick the state from "Available states" whose `group` is `completed` and whose name best matches the workflow.

2. **If the run is blocked** (missing auth, missing access, or a decision only a human can make):
   - Wrote the final workpad noting the blocker and setting `Awaiting human reply` to point at the comment.
   - Posted a comment to the human with the question or blocker, written as a colleague would (see "Blocking the run" and "Analyze & scope" for tone).
   - If the project has a state whose name is "Blocked" (see "Available states"), move the issue there:
     `pidash issue patch {{ issue.identifier }} --state "Blocked"`
     If no "Blocked" state exists, leave the issue in its current state. The comment to the human is the signal.

3. **If the issue was already terminal or not workable** (you were invoked on a `completed` / `cancelled` / `backlog` issue, or there is genuinely nothing to do):
   - Post a single short comment explaining what you observed.
   - Do not move state.

The authoritative record of this run lives on the issue: its state, the workpad, and any comments you posted. The runner reports only process `exit_code` and elapsed seconds to the cloud.
