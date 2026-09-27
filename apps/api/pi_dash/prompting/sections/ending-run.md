---
key: ending-run
title: Ending the run
customizable: locked
---
## Ending the run

There is no fenced `pi-dash-done` block. The cloud does not parse your final turn message. A run ends when the agent process exits; Pi Dash learns the outcome from the issue itself and from the one report you make with `pidash run yield`. Before your final turn ends you must have already done all of the following via `pidash`:

### Report the outcome — `pidash run yield` (always, as your last `pidash` call)

The ticking clock reads exactly one signal from a run: the outcome you report. Without it the clock guesses (it keeps ticking for In Progress; it stops for review / test). Report it after the state move, as the last thing you do:

```sh
pidash run yield --outcome <progressed|waiting_on_human|waiting_on_external|done|blocked> [--note "<one line>"]
```

- `progressed` — you did work and there is more to do in this stage; tick again.
- `waiting_on_human` — you asked the human a question, or the budget is spent and you told them; the clock stops until a human acts.
- `waiting_on_external` — In Progress is waiting on CI or a merge; keep ticking.
- `done` — this stage's exit condition is met: you moved the issue on, **or** it is approved / verified and stays where it is. The clock for this stage stops; the next stage's run (if any) is already queued.
- `blocked` — you cannot proceed; you followed "Blocking the run".

The report is tied to this run (`PIDASH_RUN_ID` in your environment); a run that has already moved the issue on is never mistaken for the next stage's run, so the order of "move" and "yield" does not matter for correctness — but do both.

### Workpad completeness — verify before exit (always, regardless of outcome)

The workpad is the only carrier of state into the next run. The next agent will start from a fresh session with no memory of this one — anything you do not write here is lost. Before exit, confirm each of the following on the workpad (the body you `pidash workpad update`'d most recently):

- [ ] `### Path to done` is current: `Stage`, `History` (appended, not rewritten), `Next state` with the reason, `Open items` for the next run (in any stage).{% if run.kind == "coding-task" %}
- [ ] `### Phase` reflects the current state (not stale from a prior run).
- [ ] `### Progress Checkpoints` match what is actually true in the repo and the issue right now. Items that don't apply to this task are marked `n/a` (e.g. `- [x] pr_opened (n/a)`), not left unchecked.
- [ ] `### Analysis` is populated: `Restated problem`, `Acceptance criteria`, `Proposed approach`, `Task type`, `Risks / assumptions`, `Decision`. No placeholder text.
- [ ] `### Plan` reflects the current plan, with checked-off items reflecting current reality. New scope discovered this run is added; obsolete items are removed or marked done.
- [ ] `### Notes` captures anything material learned this run that the next run needs to know — non-obvious decisions, dead ends ruled out, environment quirks, file/symbol pointers used.
- [ ] If exiting in a blocked state (see §2 below): `### Autonomy / Escalation` has `safe_to_continue: false`, `Reason:` explains why, and `Awaiting human reply:` carries a one-line note (date + gist) pointing at the comment you posted. This is a self-note for the next run — no code reads it; the run that picks up after the human replies should still confirm by reading the recent comments.{% else %}
- [ ] The implementation sections (`Phase`, `Progress Checkpoints`, `Analysis`, `Plan`, `Validation`, `Notes`) are carried forward **unchanged** — they belong to In Progress.{% endif %}

If any item fails this check, fix the workpad before exiting — `pidash workpad update --body-file <path>`. A workpad missing required fields means the next tick has to redo investigation or re-ask the human, which is the failure mode this discipline prevents.


1. **If the work completed successfully:**
   - Pushed your branch (if any code changed).
   - Wrote the final workpad via `pidash workpad update` so its checkpoints and validation notes are accurate.
   - Optionally posted a short completion comment (e.g. "Done — PR <url> ready for review") if the human will benefit from the ping; otherwise the issue state change is signal enough.{% if run.kind == "coding-task" %}
   - Posted the **acceptance criteria + test instructions** hand-off comment (`### Acceptance Criteria` / `### How to Test`) — see "Implementation & validation" Step 7. The test phase runs in a fresh session and inherits nothing but the issue; without this comment it has no spec to test against.{% endif %}
   - Moved the issue to the correct next state — see "Task lifecycle" for the exit conditions and allowed moves.{% if run.kind != "review" and run.kind != "test" %} **The runner never proactively moves an issue to `completed`/"Done" — read this carefully, it is the single most common place runs get the ending wrong:**
     - **Move a successfully finished issue to the `review` group — usually "In Review" — whether or not you opened a PR.** A `code_change` that opened a PR is awaiting human review and merge; a finished `noncode` task (a question answered in a comment, a debug/investigation with the root cause posted, a status check) is awaiting a human's acknowledgement. In **both** cases the runner's job is done but the *issue* is not — a human (or a separate supporting process) closes it to Done. Marking it "Done" yourself drops it off the user's radar prematurely.
       `pidash issue patch {{ issue.identifier }} --state "In Review"`
       Pick the state from "Available states" whose `group` is `review`. **Only** if this project exposes no `review`-group state at all, leave the issue in its current state and let the human move it.
     - So for the classic examples — "what color is the home page button?" or "help debug why X" — answer fully in an issue comment, then move the issue to **In Review**. Do **not** move it to Done just because you finished answering.{% elif run.kind == "test" %} Resolve the destination from this test pass's outcome — see "Test cycle" Step 3: a **pass** posts its results comment and **leaves the issue In Test** (the runner never moves it to Done); **defects** go **back to In Progress** with open items listed on the workpad; **cannot run** follows "Blocking the run"; a **clarification** follows "Blocking the run". Match the target `group` first in "Available states", then the name.{% else %} Resolve the destination from this review pass's outcome — see "Review cycle" Step 3: an **approved** review posts its summary and moves the issue to **In Test** (or leaves it In Review if the project has no `test` state; the runner never moves it to Done); **changes needed** goes **back to In Progress** with open items listed on the workpad; a **clarification** follows "Blocking the run". Match the target `group` first in "Available states", then the name.{% endif %}

2. **If the run is blocked** (missing auth, missing access, or a decision only a human can make):
   - Wrote the final workpad noting the blocker and setting `Awaiting human reply` to point at the comment.
   - Posted a comment to the human with the question or blocker, written as a colleague would (see "Blocking the run"{% if run.kind == "coding-task" %} and "Analyze & scope"{% endif %} for tone).
   - If the project has a state whose name is "Blocked" (see "Available states"), move the issue there:
     `pidash issue patch {{ issue.identifier }} --state "Blocked"`
     If no "Blocked" state exists, leave the issue in its current state. The comment to the human is the signal.

3. **If the issue was already terminal or not workable** (you were invoked on a `completed` / `cancelled` / `backlog` issue, or there is genuinely nothing to do):
   - Post a single short comment explaining what you observed.
   - Do not move state.

The authoritative record of this run lives on the issue: its state, the workpad, any comments you posted, and the outcome you reported with `pidash run yield`. The runner reports only process `exit_code` and elapsed seconds to the cloud.
