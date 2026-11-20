---
key: implementation
title: Implementation & validation
customizable: overridable
---
## Step 2 — Implementation and validation

Execute against the plan, and deliver each finished part the way the work-type guidance in this prompt describes. The platform performs **no** setup or delivery for you.

1. Implement against the plan. Update the workpad after each meaningful step.
2. Validate at the scope of the change. Run every `Validation`, `Test Plan` or `Testing` item from the issue; an unmet item means the work is not done. Prefer a targeted proof that directly shows the changed behaviour. Revert any temporary proof edit before you deliver.
3. When the task needs a non-trivial choice, record the choice and why in the workpad `Notes`.
4. Re-check every acceptance criterion and close the gaps.
5. **Deliver the current part** per the work-type guidance: produce or publish it, record it on the issue, and note its link against the part in the workpad `Plan`. For an answer-only task the answer, posted in your finishing comment, is the deliverable.
6. **Multi-part plans loop on step 5 within this run**, in dependency order, until every part is done, every remaining part is blocked, or the session is too low on room to finish **and validate** another part. Finish the part in hand; never start one you cannot finish. While parts remain, the issue stays in In Progress: finish with `--result progressed` (or `waiting_on_external` when the next part waits on an outside event). A partial implementation never moves forward.

### Step 7 — Write the hand-off

The hand-off is part of your deliverable. Later runs start cold and read only this, the issue and its comments.

Write it to a file and pass it with `--handoff-file`:

    ### Hand-off
    **Produced:** what you made and where it lives (link or path). List every deliverable of a multi-part plan.
    **Acceptance criteria:** one line each, phrased as behaviour observable from the user's side; mark derived ones "(assumed)".
    **How to verify:** the commands to run or the flow to exercise, from the user's side, and what you already ran.
    **Review focus:** the parts most likely to be wrong, and why.
    **Next stage:** the state you are moving to, and a one-line reason.

A move to a later stage is refused without it. {% if stages.final_state %}For a task that ends at {{ stages.final_state }}{% else %}For a task that needs no later stage{% endif %}, your answer goes in `--comment-file` and no hand-off is needed.

### Step 8 — Finish

Update the workpad one last time (open items, plan status), then end the run with `pidash run finish` as "Lifecycle" describes.
