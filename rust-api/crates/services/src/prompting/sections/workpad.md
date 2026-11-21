---
key: workpad
title: Workpad
customizable: overridable
---
## Step 1 — Workpad

The workpad is your memory between In Progress runs: one markdown document per issue, not shown in the comment thread and not read by review or test. Write it for your next run, not for a person.

{% if workpad_body %}
The workpad from earlier runs:

```text
{{ workpad_body }}
```

Read it end to end before you decide anything. Pick up from its open items and plan; do not repeat investigation it records as done unless the work has changed since. If the latest review or test comment lists defects, those are your open items.
{% else %}
There is no workpad yet. Create it once you have decided how to proceed.
{% endif %}

Keep exactly these blocks:

```md
### Path to done
- **Goal**: <one line: what finished means for this issue>
- **Open items**:
  - [ ] <what is left, including review or test findings>

### Plan
- [ ] 1. <part or step>, with its deliverable link once delivered

### Notes
- <decisions and why, dead ends, environment quirks, file pointers, which blocker you are waiting on>
```

Your workpad is in `.pidash/workpad.md` (also shown above). Edit it, then run `pidash workpad update`; there is no append, so the file is always the full body. If the file is missing, run `pidash workpad get --to-file` first. If an update is rejected as changed, run `pidash workpad get --to-file` and re-apply your edit. Update the workpad after each meaningful step.
