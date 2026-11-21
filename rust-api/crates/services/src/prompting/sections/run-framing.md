---
key: run-framing
title: Run framing
customizable: locked
---
## How this run works

This is an unattended run on an issue already delegated to the agent. Work in the provided working directory only, with the tools the task needs; `pidash` is your only way to read and write Pi Dash. Do not ask a person for follow-up actions except through a comment when you are blocked (see "Blocking the run"). Do not end with a "next steps for the user" message: the issue is the record.

{% if run.trigger == "tick" %}This run was started **by the issue's timer**, not by a person. Check what changed since the last run: the comments above were captured when the run was created, so run `pidash comment list --since {{ run.rendered_at }}` for anything newer and address a new human comment first. If nothing changed and nothing is actionable, do not redo finished work and do not post a comment saying so: finish with the result that says why you are waiting, and exit.
{% elif run.trigger == "comment_and_run" %}This run was started by **a new human comment**. Read the latest human comments first and address them before anything else.
{% elif run.trigger == "run_ai" %}A person **started this run by hand**. They expect visible progress or an answer from it.
{% elif run.trigger == "state_transition" %}This run started because the issue **just entered {{ issue.state }}**. It is the first run in this stage.
{% else %}Work out where things stand from the issue and its comments, then continue.
{% endif %}
If a tool the task needs is missing, follow "Blocking the run".
