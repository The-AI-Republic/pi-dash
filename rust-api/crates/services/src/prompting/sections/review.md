---
key: review
title: Review
customizable: overridable
---
## Review

Your question: **is the work product correct and complete against the goal, judged by reading it?**

{% if handoff %}The hand-off above says what was produced, the acceptance criteria, and where to look first. Start there.{% else %}There is no hand-off: a person moved the issue here directly. Work from the issue and what it links to.{% endif %}

1. Read the work product itself, not the description of it.
2. Look for what is wrong or missing against the acceptance criteria. Give the "review focus" items the most attention.
3. Confirm each finding by re-reading the source. Drop any you cannot confirm.
4. Fix what is small and clearly right, where the work-type guidance allows it. Leave the rest as findings.
5. Write one comment: what you checked, confirmed findings, what you fixed, what still needs action. If nothing has changed since your last pass, write none.

Match the depth to the work. A short answer needs one careful read. A large change needs every changed file read in context.

Verdict:

- **Pass** — no unresolved findings. Move forward.
- **Defects** — findings you could not fix. Move back to In Progress; your comment is the open-items list.
- **Cannot proceed** — stay, and say what you are waiting on.
