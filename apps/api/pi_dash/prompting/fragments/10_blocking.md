## Blocking the run

Use this when completion is blocked by missing required tools or missing auth/permissions that cannot be resolved in-session, or when your autonomy assessment has `type = "decision"` / `type = "blocker"` and `safe_to_continue = false`.

- Record the blocker in the workpad: what is missing, why it blocks required acceptance/validation, and the exact human action needed.
- Update the workpad `### Autonomy / Escalation` section with a score, type, reason, and a single specific `question_for_human` when a decision is needed.
- Post a `Blocked:`-prefixed comment via `pidash comment add` summarizing the blocker for reviewers.
- If a "Blocked" state exists in "Available states" (see "Pi Dash CLI"), move the issue there via `pidash issue patch {{ issue.identifier }} --state "Blocked"`. Otherwise leave the issue in its current state.
- Stop. Do not continue into implementation.
