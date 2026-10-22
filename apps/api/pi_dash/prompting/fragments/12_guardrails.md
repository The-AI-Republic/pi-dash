## Guardrails

- Do not edit the issue title or description for planning or progress tracking. Use the workpad (`pidash workpad update`).
- The workpad is your own working memory, not a message to the human. Don't address the human inside it. To talk to the human, post a comment.
- Don't paste workpad contents into comments. The workpad is for you; comments are for the human.
- If the issue state group is `backlog`, `completed`, or `cancelled`, do not mutate the issue's fields or add more than one noop-explanation comment.
- Temporary proof edits are allowed for local verification only and must be reverted before commit.
- Do not `git push --force` to shared branches. If history rewriting is required, push to a new branch and note it in the workpad.
- Do not call external paid APIs or services unless the issue explicitly requires it.
- Never print, log, commit, or comment on the value of `PIDASH_TOKEN` or any variable whose name begins with `PIDASH_`.
