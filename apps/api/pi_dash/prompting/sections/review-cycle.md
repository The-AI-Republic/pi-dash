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
name), then the outcome report from "Ending the run". The outcome
describes this run — `done` means "this run's turn is done" — and only an
explicit `--stop-ticking` stops the issue's clock:

- **approved** — no unresolved findings. Post your summary comment and
  move the issue to **In Test** (the `test` group) so the change gets
  exercised from the user's side. Yield `done` — your turn is done and
  the issue moved on.
- **approved, no `test` state** — the project has no `test`-group state
  to move on to. Leave the issue In Review (never move it to
  `completed`/Done — a human closes it) and yield `done --stop-ticking`:
  the review stage is satisfied, so no further review run has anything
  to add until a human acts.
- **changes needed** — real defects you could not auto-fix. List them as
  open items in the workpad `### Path to done` block, post the summary
  comment, and move the issue **back to In Progress** (the `started`
  group) so the next run fixes them. Do not use Blocked for a defect.
  Yield `done`.
- **clarification** — a question only a human can answer. Follow
  "Blocking the run". Yield `waiting_on_human --stop-ticking`.
- **waiting on an external process** — automated checks are still
  running, an acceptance or update of the deliverable is pending, or
  nothing has changed since your last pass while that remains true.
  Leave the issue In Review and yield `waiting_on_external` —
  **keep the clock ticking** so a later run re-checks; no human is
  needed for this wait. Comment only if you have something a human actually needs to see
  (a finding, a question, a result); do **not** post a bare "review tick
  (N/M) — noop, nothing changed" comment — silence is the correct signal
  for "nothing changed," and such comments only bury the ones that
  matter.
- **waiting on a human reviewer** — the work product waits on a person
  (a requested review, a human decision on your findings), or nothing
  has changed since your last pass and that person is the only way
  forward. Leave the issue In Review and yield
  `waiting_on_human --stop-ticking` — no further review run can do
  anything useful until they act; their reply, a state move, Run AI or
  Re-tick re-arms the clock. Same comment discipline: silence unless a
  human needs something from you.
