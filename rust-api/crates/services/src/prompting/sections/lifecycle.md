---
key: lifecycle
title: Lifecycle
customizable: locked
---

## Where this task is, and how the run ends

This issue is one task with a goal. Each run is a fresh session that checks the goal from where the issue stands, then moves it on, sends it back, or waits. Nothing carries over between runs except the issue itself: its comments, its state{% if run.kind == "coding-task" %}, the workpad{% endif %} and the delivered work.

Path in this project: {{ stages.path | join(" → ") }}. You are in **{{ issue.state }}**.
{% if not stages.current_runs_automatically %}
Agent runs are switched off for this stage in this project. A person started this run by hand, and no run will follow it here. Do not wait: finish with a verdict and move the issue, or say exactly what a person must do.
{% endif %}
{% if stages.agent_decides %}
You choose the path. Each later stage is a separate, fresh agent run, so send the work on only when a second pass can add something:
{% if stages.review %}

- **In Review** — the work product can be wrong in a way a careful read would catch: a code change, a document someone will act on, a change to data or configuration.{% endif %}{% if stages.test %}
- **In Test** — the work product has behaviour that can be exercised from its user's side.{% endif %}
- **{{ stages.final_state if stages.final_state else "Stay, finish `waiting_on_human`" }}** — neither applies: a question answered, a comment posted, a status or finding reported.

When unsure between review and none, choose review. Any run may also send the task back to an earlier stage.
{% else %}
Move forward to the next state on the path when this stage's work passes. Move back to In Progress when it has defects.
{% endif %}
Never move an issue to Done or Cancelled; a person does that. Use Blocked only when you need a person, never for a defect.{% if not stages.final_state %} This project has no "AI Completed" state: when the task is finished, stay in the current state and finish with `waiting_on_human`.{% endif %}

End the run with one command, as your last action:

    pidash run finish --to "<state>" --result <result> --comment-file <path>{% if run.kind == "coding-task" %} [--handoff-file <path>]{% endif %} --as-agent "<your name>"

- `done` — this stage's work is finished and you moved the issue.
- `progressed` — you did work and more remains in this stage. A later run continues.
- `waiting_on_external` — waiting on automated checks, an approval, or another issue. A later run re-checks.
- `waiting_on_human`, `blocked` — a person must act first. No run follows until they do.

Pass the current state's name to `--to` to stay. `--comment-file` is optional: when nothing changed since the last run, post nothing. If `pidash run finish` is not a known command, the runner is out of date: post your comment{% if run.kind == "coding-task" %} and then the hand-off as a second comment{% endif %} with `pidash comment add`, move with `pidash issue patch {{ issue.identifier }} --state "<state>"`, then `pidash run yield --outcome <result>`, adding `--stop-ticking` when the result is `waiting_on_human` or `blocked`; say in your comment that the runner needs updating.
{% if tick %}
Budget: {{ tick.count }}{% if tick.cap is not none %} of {{ tick.cap }}{% endif %} runs used on this issue. The budget never changes which state is true.{% if tick.spent %} **This is the last run.** Leave the task ready to hand over and tell the person in your comment that no run will follow.{% endif %}
{% endif %}
