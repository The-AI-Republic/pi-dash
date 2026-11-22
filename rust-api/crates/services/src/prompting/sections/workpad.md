---
key: workpad
title: Workpad
customizable: overridable
---
## {% if run.kind == "coding-task" %}Step 1 — {% endif %}Workpad

The workpad is the shared memory of the agent runs on this issue: a short note of what happened and what is true, like a shift note on a production line. It is memory, not instructions. Every run on this issue starts from it. It is for agents only and never shown in the comments: write what the next run needs and a person does not. The issue and comments are already in front of the next run, so do not copy them.

{% if workpad_body %}
The workpad so far:

```text
{{ workpad_body }}
```

Read it first, and do not redo what it says is done. If it uses an older layout, rewrite it in the shape below on your first update, keeping only what is still true.
{% else %}
There is no workpad yet. Start one in this shape.
{% endif %}

```md
### Keep in mind
- <only what the next run must not have to rediscover>

### Log
- <YYYY-MM-DD> <stage>: <one line: what this run did>
```

**Keep in mind** holds, one short bullet each: where the work is and how to check it; what is still open, as `Open: …`; decisions the next run might otherwise undo; traps and the commands that actually work. Write facts, never instructions for the next run: its own prompt tells it what to do. Change or remove bullets that are no longer true.

Each run that does something: update **Keep in mind** and add one **Log** line, keeping the last ten. A run that only finds nothing changed adds nothing.{% if run.kind == "coding-task" %} A move to review or test is refused until this run has updated the workpad.{% endif %} Keep it to about 40 lines, with no pasted output.

Your workpad is in `.pidash/workpad.md` (also shown above). Edit it, then run `pidash workpad update`; the file is always the full body. If the file is missing, run `pidash workpad get --to-file` first. If an update is rejected as changed, run `pidash workpad get --to-file` and re-apply your edit.
