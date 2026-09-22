## Blocking the run

Use this when completion is blocked by missing required tools or missing auth/permissions that cannot be resolved in-session, or when your autonomy assessment has `type = "decision"` / `type = "blocker"` and `safe_to_continue = false`.

1. **Update the workpad** (`pidash workpad update --body-file <path>`):
   - `### Phase` left at the phase you were in, not "completed".
   - `### Autonomy / Escalation` updated: `score`, `type`, `safe_to_continue: false`, `Reason:` explaining the blocker in your own words.
   - `Awaiting human reply:` set to a one-line note about the comment you're about to post (gist + today's date). This is a self-note for the *next* run — it isn't read by any code on the cloud side, but reading it as part of your workpad reconciliation in Step 1 tells you immediately that you're resuming a clarification thread (so you check the most recent human comments first) rather than starting a fresh investigation.

2. **Post a comment to the human** via `pidash comment add {{ issue.identifier }} --body-file <path>`. Write as a colleague (see Step 0.5 step 7 for tone). Lead with what's blocking, then what you need from them. Don't use a `Blocked:` prefix or any other template — just write it.

   Example (missing access):

   > Blocked: I need the staging Postgres credentials to verify the migration in §2 runs cleanly before I open the PR. Could you drop them into the `pi_dash_runner` 1Password vault, or rotate me a short-lived token? Once that's in place I'll pick this back up on the next tick.

   Example (decision only a human can make):

   > One decision before I implement: the spec says "rate-limit the endpoint" but doesn't say at what tier. The existing API endpoints in `apps/api/pi_dash/api/views/issue.py` use `ApiKeyRateThrottle` (per-key) — should I apply the same here, or do you want a per-IP throttle? Defaulting to per-key matches the surrounding code, so I'll go with that in ~24h if I don't hear back.

3. **Move issue state if a "Blocked" state exists** in "Available states" (see "Pi Dash CLI"):
   `pidash issue patch {{ issue.identifier }} --state "Blocked"`
   If no "Blocked" state exists in this project, leave the issue in its current state. The comment is the signal.

4. **Stop.** Do not continue into implementation. Exit the run cleanly. The next agent tick fires automatically when the human replies (the cloud's `handle_issue_comment` listener queues a follow-up run on any non-bot comment); that run will start fresh, read the full comment thread via `pidash comment list`, see the human's reply, see your `Awaiting human reply` self-note in the workpad, and continue from where you left off.
