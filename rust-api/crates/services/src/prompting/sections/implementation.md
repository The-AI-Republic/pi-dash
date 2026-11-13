---
key: implementation
title: Implementation & validation
customizable: overridable
---

## Step 2 — Implementation and validation

Execute against the plan, and deliver each finished part the way the work-type guidance in this prompt describes — the platform performs **no** setup or delivery for you; every step is yours to run.

1. Implement against the hierarchical TODOs. Update the workpad after each meaningful milestone and keep `### Phase`, `### Progress Checkpoints`, and `### Autonomy / Escalation` current:
   - `investigation_complete`
   - `design_choice_recorded`
   - `implementation_complete`
   - `validation_complete`
   - `delivered`
   - `feedback_addressed`
     Treat `delivered` and `feedback_addressed` as optional checkpoints. For tasks that produce no durable deliverable or do not enter review, mark them `n/a` rather than leaving them falsely incomplete.
2. Run validation appropriate to the scope.
   - Execute every ticket-provided `Validation`, `Test Plan`, or `Testing` item. Unmet items mean the work is incomplete.
   - Prefer a targeted proof that directly demonstrates the behavior you changed.
   - Temporary local proof edits (e.g. hardcoding a value to validate a UI path) are allowed **only** for local verification and must be reverted before you deliver.
3. When the task requires a non-trivial technical choice, record the selected approach and rationale in the workpad `Notes`, set the autonomy assessment accordingly, and proceed only if `safe_to_continue` is `true`.
4. Re-check all acceptance criteria. Close any gaps.

   **For a multi-part plan, steps 5–6 are a loop.** If your "Analyze & scope" decision was *Proceed with a multi-part plan*, or a `### Plan` with more than one part is already recorded in the workpad from an earlier run, you deliver **one part at a time for reviewability** — but you do **not** stop after the first. Repeat steps 5–6 for each part of the workpad `### Plan` that isn't blocked, in dependency order, within this **same run**, each part building on the previous one when it depends on it (the work-type guidance says how). **End the run when any one of these is true:**
   - all parts are done; or
   - every remaining part is blocked — waiting on a human decision, or on a delivered part that must be accepted before the next part can build; or
   - the session is running low on room to finish **and validate** another part.

   In that last case, finish the part in hand — delivered per the work-type guidance, workpad updated — and stop there; never start a part you cannot finish. A single-deliverable issue is simply the one-part case: steps 5–6 run once. What changes is how many parts a run delivers, not the "one deliverable per part" rule.

5. **Deliver the current part** following the work-type guidance in this prompt: produce/publish the deliverable, record it on the issue (the guidance says on which surface), and mark `delivered` in the workpad — for a multi-part plan, also record the deliverable's link against this part's entry in the workpad `### Plan`. Skip this step for an answer-only task — the answer or action itself, posted as an issue comment, is the deliverable, and `delivered` is `n/a`.
6. **If unblocked parts of the plan remain, go back to step 5 for the next part.** Do not advance the stage, do not post the testing hand-off, and do not yield `done` while parts are still unbuilt — a partial implementation must not move the issue to In Review (that fires a review run on work that can only report "not done"). Continue to step 7 only once every part is built, or you are ending the run with parts still blocked or unfinished (per the loop's end conditions above).

7. **Hand off to testing — once, for the whole issue, when every plan part is built.** Write the acceptance criteria to the workpad `### Path to done` block (the canonical copy every later run reads) and post them with the test instructions as an issue comment (the human-readable mirror). Do this only after the issue's implementation is complete — for a multi-part plan that means **all** parts are built and the issue's acceptance criteria are covered, not after a single part. While parts remain, a short per-part note in the workpad `### Plan` is enough; skip this hand-off until the run that finishes the last part. Skip it entirely only for a pure `noop`. A test pass is only as good as the spec it tests against, and the In Test phase starts from a **fresh session** with no memory of this run — this comment is the spec it inherits. When the issue was delivered as several parts, **list every deliverable** here so the test phase exercises them together. Post it with `pidash comment add {{ issue.identifier }} --body-file <path>` using exactly these headings so it can be found and parsed later:

   ```
   ### Acceptance Criteria
   - <one checkable criterion per line — the conditions that make this work correct>
   - <criteria you derived rather than found in the ticket: mark "(assumed)">

   ### How to Test
   - Kind: <your best call on how this deliverable is verified — the work-type guidance names the kinds>
   - Setup: <how to obtain the work product, per the work-type guidance; services/env/creds needed; seed data>
   - Steps: <the exact commands to run or flows to drive, in order>
   - Expected: <what a pass looks like, per criterion>
   - Already validated here: <what you actually ran this run, and its result>
   - Not covered: <gaps you knowingly left — and why>
   ```

   Write the criteria as things that can be **checked from the user's side of the surface** — the test phase verifies them by impersonating whoever consumes this change (a human in the UI, a program calling the API, an operator running a procedure, a reader acting on a document), so phrase each criterion as observable behavior, not as a summary of what you built ("`pidash issue patch` rejects an unknown state name with exit code 2", not "improved error handling"; "the sidebar shows the review interval field", not "added the field to the sidebar component"). `Kind` is your best call on how this deliverable is verified; the test agent re-derives it and may disagree. If you genuinely could not establish acceptance criteria, say so under the heading rather than omitting it — an explicit "none stated; derived from the description" tells the test pass to state assumptions instead of inventing a spec.

8. Update the workpad with final checklist status and validation notes. Add a `### Confusions` section at the bottom if anything about the task was genuinely unclear during execution; keep it concise.
9. **Choose the next state from where the plan stands, then follow "Ending the run" to finalize.** Update the workpad one last time with final checkpoints and the `### Path to done` block (stage, history, next state and why, open items). Then:
   - **Plan parts still remain** (unbuilt parts of a multi-part plan): the issue **stays In Progress** — do not move it to In Review and do not post the testing hand-off. Report `pidash run yield --outcome progressed`, or `waiting_on_external` when the only thing left is an external event (e.g. a delivered part awaiting acceptance) before the next part can build. The next run reads the workpad `### Plan` and continues where you left off.
   - **All parts are built and the issue's acceptance criteria are covered** (a single-deliverable issue reaches this immediately): post the one whole-issue testing hand-off from step 7 (listing every deliverable), then move the issue to its next state via `pidash issue patch {{ issue.identifier }} --state "<state-name>"` and report `pidash run yield --outcome done`. **A finished issue moves to the `review` group ("In Review") whether it produced a durable deliverable or only an answer — the runner never proactively moves an issue to `completed`/"Done".** A deliverable is awaiting human review; a finished answer-only task (a question answered, a debug/investigation posted) is awaiting a human's acknowledgement — leave it In Review so it stays on the user's radar, and let a human close it. See "Ending the run" for the exact rule and the no-`review`-state fallback.
