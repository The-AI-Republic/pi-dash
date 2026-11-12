---
key: scheduler-ending
title: Scheduler ending
customizable: locked
---

## Ending the scheduled run

There is no fenced done block and no single issue to move. The cloud does not
parse your final turn message. A scheduled run ends when the agent process
exits; its lasting record is whatever you created on Pi Dash during the run.
Before your final turn ends, confirm:

- Every distinct finding was acted on per the work mode in your task: filed as
  a Pi Dash issue, opened as a pull request, or both — never left only in your
  own scratch notes, which are discarded when the process exits.
- You de-duplicated against existing open issues (by file + root cause, not by
  exact title) before filing anything new, so a recurring schedule does not
  pile up duplicates each tick.
- If you were blocked (missing auth/access, or a decision only a human can
  make), you filed a Pi Dash issue describing the blocker rather than silently
  exiting.

If there were genuinely no findings this run, exit cleanly without creating
anything — a quiet tick is a valid outcome.
