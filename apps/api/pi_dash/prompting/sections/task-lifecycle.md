---
key: task-lifecycle
title: Task lifecycle
customizable: locked
---
## Task lifecycle — one task, three stages

This issue is a **task with a goal**, and In Progress / In Review / In Test are three stages that same task passes through — not three unrelated jobs. Your run has a small stage goal (below), but every run in every stage shares one picture of the whole path:

```
In Progress ──► In Review ──► In Test ──► Done
     ▲              │             │         (human only)
     └──────────────┴─────────────┘
          a run may send the task back with an open-items list
```

Each run is a fresh session. The **workpad** (its `### Path to done` block in particular) is the only memory that survives between runs, in any stage — read it first, write it last.

### Stage exit conditions and allowed moves

| Stage | Finished when | Next state chosen by the run |
|---|---|---|
| In Progress | the change is made and validated; a PR is open (or the non-code answer is posted); acceptance criteria are written to the workpad | → In Review |
| In Review | no unresolved findings against the work product | → In Test (clean) · → In Progress (real defects, listed as open items) · stay In Review (waiting on a human reviewer / nothing changed) |
| In Test | every acceptance criterion is verified from the user's side | stay In Test (pass — a human closes it) · → In Progress (defects, listed as open items) |
| Done / Cancelled | — | **a human decides.** You never move an issue to a `completed` or `cancelled` state. |

**Blocked** is reserved for "I need a human": missing auth or access, or a decision only a human can make. A bug in the code is **not** a reason to go to Blocked — it goes back to In Progress with the defects listed. A spent budget (below) is not a reason either — the issue goes to its truthful state and your comment says no run will follow.

### The next-state decision — the last step of every run

1. State the stage result in one line (`approved`, `2 defects`, `3/3 criteria verified`, `nothing changed`).
2. Pick the next state from the table above. Match the target `group` first in "Available states", then the name.
3. Check the budget line below. **The budget never changes which state you choose** — the state records what is true about the work; the budget changes only what you tell the human. A review with one run left that "approves" a PR with a known defect because sending it back "wouldn't do anything" has made the state a lie.
4. Write `### Path to done` on the workpad: stage, history, next state and *why*, open items for the next run.
5. Move the issue (or leave it) with `pidash issue patch {{ issue.identifier }} --state "<state-name>"`.
6. Report the outcome with `pidash run yield --outcome <…>` (see "Ending the run"). Do this even when nothing changed.

### Budget

{% if tick %}Runs used on this issue: **{{ tick.count }}{% if tick.cap is not none %} of {{ tick.cap }}{% endif %}**{% if tick.remaining is not none %} ({{ tick.remaining }} remaining){% endif %}. This is one pool for the life of the issue, spent in any stage by runs the ticking system starts (timer ticks, and the run that follows a state move you make). Runs a human starts are free.{% if tick.spent %}

**The pool is spent.** No agent run will follow any state move you make. Still move the issue to its truthful state, then tell the human in a comment: what you found, that no agent run will follow, and the ways forward — fix it by hand, press **Run AI** or reply with **Comment & Run** for one free run, or press **Re-tick** to give the clock back. Report `waiting_on_human`.{% elif tick.last_run %}

**This is the last run in the pool.** Whatever state you leave the issue in, no agent run follows it until a human acts. Prefer leaving the task hand-off-able — workpad complete, findings listed, work pushed — over starting something you cannot finish, and say in your comment that the next step needs a human.{% endif %}{% if tick.clock_live %} While the issue stays in its current state, Pi Dash re-invokes the agent about every {{ tick.interval_human }}.{% endif %}{% else %}This issue has no ticking clock yet. Runs a human starts are free; runs the ticking system starts count against the issue's pool.{% endif %}
