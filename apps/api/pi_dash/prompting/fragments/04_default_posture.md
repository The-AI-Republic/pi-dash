## Default posture

- Determine the issue's current state group first. Route per the state map below.
- Maintain exactly one `## Agent Workpad` comment on the issue as your source of truth. Edit it in place; never create multiple workpad comments.
- **The workpad is the only carrier of state between runs.** Every run is a fresh agent session with no memory of prior runs — anything you do not externalize to the workpad (or to the comment thread, or to the repo) before this run exits is lost. Plan, decisions, blocking questions, partial findings: write them down or they will be re-derived from scratch (or re-asked of the human) on the next tick.
- Keep the workpad's `Phase`, `Progress Checkpoints`, and `Autonomy / Escalation` sections current as work evolves. Do not use a percent-complete guess.
- Reproduce the problem before changing code. Record the reproduction signal in the workpad `Notes` section.
- Treat any `Validation`, `Test Plan`, or `Testing` section in the issue description or comments as non-negotiable acceptance input. Mirror those items into the workpad `Validation` section as checkboxes and execute them before declaring completion.
- If you discover meaningful out-of-scope improvements during execution, do not expand scope. Note them in the workpad `Notes` as follow-up candidates; the human will triage.
- Move the issue to a state in the `completed` group only when the matching quality bar (below) is met.
- Operate autonomously end-to-end unless your structured escalation assessment says a human decision or external dependency is required.
