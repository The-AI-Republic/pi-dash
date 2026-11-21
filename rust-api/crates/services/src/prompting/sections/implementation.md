---
key: implementation
title: Implementation & validation
customizable: overridable
---
## Step 2 — Implementation and validation

Execute against the plan, and deliver each finished part the way the work-type guidance in this prompt describes. The platform performs **no** setup or delivery for you.

1. Implement against the plan. Update the workpad when something happens the next run must not lose.
2. Validate at the scope of the change. Run every `Validation`, `Test Plan` or `Testing` item from the issue; an unmet item means the work is not done. Prefer a targeted proof that directly shows the changed behaviour. Revert any temporary proof edit before you deliver.
3. When the task needs a non-trivial choice, note the choice and why in the workpad's Keep in mind.
4. Re-check every acceptance criterion and close the gaps.
5. **Deliver the current part** per the work-type guidance: produce or publish it, record it on the issue, and put its link in the workpad's Keep in mind. For an answer-only task the answer, posted in your finishing comment, is the deliverable.
6. **Multi-part plans loop on step 5 within this run**, in dependency order, until every part is done, every remaining part is blocked, or the session is too low on room to finish **and validate** another part. Finish the part in hand; never start one you cannot finish. While parts remain, the issue stays in In Progress: finish with `--result progressed` (or `waiting_on_external` when the next part waits on an outside event). A partial implementation never moves forward.

### Step 7 — Leave the note

Before you move the issue on, the workpad's Keep in mind must say where the work is (every link or path) and how to check it. A move to review or test is refused until this run has updated the workpad. {% if stages.final_state %}For a task that ends at {{ stages.final_state }}{% else %}For a task that needs no later stage{% endif %}, the answer goes in your comment.

Your comment for people, when you move the issue on: one or two lines on what you produced, with the link.

### Step 8 — Finish

Update the workpad one last time, then end the run with `pidash run finish` as "Lifecycle" describes.
