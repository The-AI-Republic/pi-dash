---
key: workpad-context
title: Workpad context
customizable: locked
---
## Workpad — read first, write last

{% if workpad_body %}
The workpad from the prior runs on this issue is shown below verbatim. It is the hand-off between runs across every stage: its `### Path to done` block carries the goal, the stage history, and the **open items** the previous run left for you. **Read it end-to-end before deciding anything**, and start from its open items rather than from the comment thread.

```text
{{ workpad_body }}
```

Before you exit, write the workpad back with `pidash workpad update --body-file <path>`: update `### Path to done` (stage, history, next state and why, open items for the next run). **Carry the rest of the body forward unchanged** — the implementation sections (`Phase`, `Progress Checkpoints`, `Analysis`, `Plan`, `Validation`) belong to In Progress; do not rewrite or drop them.
{% else %}
There is no workpad yet — no prior run has worked this issue (a human moved it straight into this stage). The work product is whatever the issue links to: an attached PR, a document, the description. Before you exit, create the workpad with `pidash workpad update --body-file <path>` containing a `### Path to done` block (goal, stage, history, next state and why, open items, acceptance criteria as you understood them) so the next run — in any stage — starts from it.
{% endif %}
