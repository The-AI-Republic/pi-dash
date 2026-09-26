---
key: review-cycle
title: Review cycle
customizable: overridable
---

## Step 1 — Decide what kind of review this is

Inspect `parent_done_payload`, the issue description, and the working
directory to identify the work product this issue produced, then choose
ONE review kind:

- The work-type guidance in this prompt defines the review kinds specific
  to this project's work type, how to recognize each, and the surface each
  one is reviewed on. Prefer those kinds when one matches.
- **GENERIC** — none of the work-type kinds fit. Review the work product
  against the issue description and leave a summary on the pidash issue.

If you cannot decide, ask the human via the "Blocking the run" flow.

## Step 2 — Run the cycle for the chosen kind

All cycles share this shape:

1. Find issues with the work product.
2. Validate your findings (no hallucinations) — re-read the artifact,
   confirm each issue is real, drop any that aren't.
3. Read existing reviewer comments (on the surface the work-type guidance
   names for the chosen kind, or on the pidash issue for GENERIC) and
   reconcile your findings against them.
4. Comment on the validated issues at the appropriate surface for the
   chosen kind (see the work-type guidance; GENERIC: a structured comment
   on the pidash issue).
5. If you can fix a confirmed issue and the kind permits it (the work-type
   guidance says which kinds do, and how), apply the fix and resolve the
   corresponding comment. GENERIC usually does NOT auto-apply — leave the
   summary and let the human act.
6. Post a summary back to the pidash issue as a comment: confirmed issues
   found, what you fixed automatically, what still needs human action.

## Step 3 — Decide where the task goes next

The review pass concludes with the next-state decision from "Task
lifecycle" (match the target `group` first in "Available states", then the
name), then the outcome report from "Ending the run":

- **approved** — no unresolved findings. Post your summary comment and
  move the issue to **In Test** (the `test` group) so the change gets
  exercised from the user's side. If the project has no `test` state,
  leave the issue In Review. Never move it to `completed`/Done — a human
  closes it. Yield `done`.
- **changes needed** — real defects you could not auto-fix. List them as
  open items in the workpad `### Path to done` block, post the summary
  comment, and move the issue **back to In Progress** (the `started`
  group) so the next run fixes them. Do not use Blocked for a defect.
  Yield `done`.
- **clarification** — a question only a human can answer. Follow
  "Blocking the run". Yield `waiting_on_human`.
- **waiting on a human reviewer / nothing changed** — the work product is
  waiting on a person, or nothing has changed since your last pass.
  Comment only if you have something a human actually needs to see (a
  finding, a question, a result); do **not** post a bare "review tick
  (N/M) — noop, nothing changed" comment — silence is the correct signal
  for "nothing changed," and such comments only bury the ones that
  matter. Leave the issue In Review and yield `done` — the clock stops
  until a human acts.
