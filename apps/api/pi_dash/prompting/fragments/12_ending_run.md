## Ending the run

There is no fenced `pi-dash-done` block. The cloud does not parse your final turn message. A run ends when Codex exits; Pi Dash learns the outcome by looking at the issue itself. Before your final turn ends you must have already done all of the following via `pidash`:

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
