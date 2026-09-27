---
key: general.execute
title: General delivery workflow
customizable: overridable
---
## General delivery workflow (work type: general)

This issue's work type is **general**: the task can be anything the project manages — research, writing, planning, operations, coordination, analysis. There is no single mandated tool chain; the deliverable is whatever artifact the task calls for, produced with the tools available in the working directory.

### Decide the deliverable first (during "Analyze & scope")

Record on the `Deliverable` line of your workpad `### Analysis` what this task will produce and **where it will live**, choosing the most durable surface the task allows:

- **A document** — a project page (`pidash page create` / `pidash page update`; list first and prefer updating an existing page over creating a near-duplicate) for knowledge the project should keep, or a file in the working directory when the operator expects one there.
- **An answer or analysis** — posted directly as an issue comment when no durable artifact is needed.
- **An action performed** — a procedure executed, a record updated; the deliverable is then the evidence of the action, summarized on the issue.

An answer-only task (question, status check) needs no separate artifact — the comment itself is the deliverable, and the `delivered` checkpoint is `n/a`.

### Deliver a part

This is what "deliver the current part" in "Implementation and validation" means here:

1. Produce or update the artifact on its surface (write the page, save the file, perform the action).
2. Record it on the issue so a human can find it from the thread: post a comment linking or naming the deliverable (page title/URL, file path, or the summary of the action and its evidence).
3. Mark `delivered` in the workpad — and for a multi-part plan, record the deliverable's link against this part's entry in the workpad `### Plan`.

### Testing hand-off specifics

In the hand-off comment's `### How to Test` block ("Implementation and validation"), `Setup` names where each deliverable lives and how to open it (page, file path, or the record that was changed), plus anything needed to verify it. When the issue was delivered as several parts, list **every** deliverable.

For `Kind`, pick from how general deliverables are verified: `DOCUMENT` (the consumer is a reader who must act on it), `ACTION / RECORD` (the consumer depends on the changed state or record), or `GENERIC` (verify the stated acceptance criteria one by one). The test phase's guidance defines each in full — name the kind here so that phase starts on the right foot.

### Guardrails

- Prefer updating an existing page or document over creating a near-duplicate.
- Do not destructively overwrite a document a human is editing; when a rewrite is needed, note what you replaced in the issue comment.
