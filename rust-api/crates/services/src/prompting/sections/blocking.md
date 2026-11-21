---
key: blocking
title: Blocking the run
customizable: locked
---
## Blocking the run

Block only when a person must act: missing tools, access or credentials you cannot get in this session, or a decision only a person can make. A defect is never a reason to block.

1. Write the comment as a colleague would. Lead with what is blocking, then exactly what you need. If there is a sensible default, name it and say you will take it unless told otherwise. One focused question beats five vague ones.{% if run.kind == "coding-task" %}
2. Record in the workpad `Notes` that you are waiting on that reply, so the next run reads the reply first instead of starting over.{% endif %}
{% if run.kind == "coding-task" %}3{% else %}2{% endif %}. Stop, and finish: `pidash run finish --to "Blocked" --result blocked --comment-file <path>` when the project has a "Blocked" state, otherwise pass the current state's name to `--to`. A person's reply starts the next run.

When what blocks you is **another issue**, do not block: say which issue and why, call `pidash issue wait {{ issue.identifier }}`, and finish with `--result waiting_on_external` in the current state. A later run re-reads the blocker and decides again.

Rules for every run:

- Do not edit the issue title or description to track progress.{% if run.kind == "coding-task" %} The one exception is a parent you split into child issues (see "Step 0 — Analyze & scope (read, think, decide)").{% endif %}
- Do not change the fields of an issue in Backlog, Done or Cancelled; post at most one short comment saying why you did nothing.
- Revert temporary proof edits before you deliver. Do not destructively rewrite work a person or another run may depend on.
- Do not call paid external services unless the issue requires it.
- Never print, log or comment the value of any variable whose name starts with `PIDASH_`.
