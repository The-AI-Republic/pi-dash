---
key: default-posture
title: Default posture
customizable: overridable
---
## Default posture

- Determine the issue's current state group first. Route per the state map below.
- Maintain the issue's workpad (`pidash workpad get` / `pidash workpad update`) as your source of truth for cross-run state. The workpad is your own working memory — it is not shown in the comment thread and is not a message to the human.
- **The workpad is the only carrier of state between runs.** Every run is a fresh agent session with no memory of prior runs — anything you do not externalize to the workpad, the comment thread, or the repo before this run exits is lost. Plan, decisions, partial findings: write them down or they will be re-derived from scratch on the next tick.
- The comment thread is the human ↔ agent conversation channel. Use it for clarifying questions, blocker notices, PR links, and completion announcements — not for tracking your own progress.
- Keep the workpad's `Phase`, `Progress Checkpoints`, and `Autonomy / Escalation` sections current as work evolves. Do not use a percent-complete guess.
- Reproduce the problem before changing code. Record the reproduction signal in the workpad `Notes` section.
- Treat any `Validation`, `Test Plan`, or `Testing` section in the issue description or comments as non-negotiable acceptance input. Mirror those items into the workpad `Validation` section as checkboxes and execute them before declaring completion.
- If you discover meaningful out-of-scope improvements during execution, do not expand scope. Note them in the workpad `Notes` as follow-up candidates; the human will triage.
- Match the issue's final state to what actually happened. A `code_change` that opened a PR moves to the `review` group ("In Review") — the work is awaiting human review and merge, not done — **not** the `completed` group. Move to `completed`/"Done" only for a finished `noncode` task with no PR to review (or when the project exposes no `review`-group state). Either way, only move once the matching quality bar (below) is met.
- Operate autonomously end-to-end unless your structured escalation assessment says a human decision or external dependency is required.
