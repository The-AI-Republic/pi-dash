---
key: implementation
title: Implementation & validation
customizable: overridable
---

## Step 2 — Implementation and validation

Before editing any files, make sure you are on the work branch — the platform performs **no** branch checkout for you; every git operation is yours to run.
{% if repo.work_branch %}This issue pins `{{ repo.work_branch }}`: commit directly onto it. You checked it out in Step 1's repository sync; if you are not on it, `git checkout {{ repo.work_branch }}` (creating it from `origin/{{ repo.work_branch }}` when it is not yet local).{% else %}No branch is pinned: derive and commit onto the fresh feature branch you created from {% if repo.base_branch %}`{{ repo.base_branch }}`{% else %}the base branch{% endif %} in Step 1 — never onto the base branch itself.{% endif %}

1. Implement against the hierarchical TODOs. Update the workpad after each meaningful milestone and keep `### Phase`, `### Progress Checkpoints`, and `### Autonomy / Escalation` current:
   - `investigation_complete`
   - `design_choice_recorded`
   - `implementation_complete`
   - `validation_complete`
   - `pr_opened`
   - `review_feedback_addressed`
     Treat `pr_opened` and `review_feedback_addressed` as optional checkpoints. For tasks that do not produce a PR or do not enter review, mark them `n/a` rather than leaving them falsely incomplete.
2. Run validation and tests appropriate to the scope.
   - Execute every ticket-provided `Validation`, `Test Plan`, or `Testing` item. Unmet items mean the work is incomplete.
   - Prefer a targeted proof that directly demonstrates the behavior you changed.
   - Temporary local proof edits (e.g. hardcoding a value to validate a UI path) are allowed **only** for local verification and must be reverted before commit.
3. When the task requires a non-trivial technical choice, record the selected approach and rationale in the workpad `Notes`, set the autonomy assessment accordingly, and proceed only if `safe_to_continue` is `true`.
4. Re-check all acceptance criteria. Close any gaps.

   **For a multi-part plan, steps 5–6 are a loop.** If your Step 0.5 decision was *Proceed with a multi-part plan*, or a `### Plan` with more than one part is already recorded in the workpad from an earlier run, you deliver **one PR per part for reviewability** — but you do **not** stop after the first. Repeat steps 5–6 for each part of the workpad `### Plan` that isn't blocked, in dependency order, within this **same run**. Stack a part's branch on the previous part's branch when it depends on it; otherwise branch from the base. **End the run when any one of these is true:**
   - all parts are done; or
   - every remaining part is blocked — waiting on a human decision, or on a PR that must merge before the next part can build; or
   - the session is running low on room to finish **and validate** another part.

   In that last case, finish the part in hand — committed, pushed, PR open, workpad updated — and stop there; never start a part you cannot finish. A single-PR issue is simply the one-part case: steps 5–6 run once. What changes is how many parts a run delivers, not the "one PR per part" rule.

5. **If `task_type == code_change`** (per your Step 0.5 analysis): commit the current part with clear, logical commit messages. Push the branch with `git push -u origin "$(git rev-parse --abbrev-ref HEAD)"`. Only after the push succeeds, persist the branch on the issue so subsequent runs land on it: `pidash issue patch {{ issue.identifier }} --git-work-branch "$(git rev-parse --abbrev-ref HEAD)"`. Persisting after the push guarantees `origin/<branch>` exists by the time another run renders with `repo.work_branch` set. Skip this step entirely for `noncode` tasks — there is nothing to commit or push.
6. **If `task_type == code_change`** (per your Step 0.5 analysis): open a {{ repo.code_review_term }} for the current part and link it back to the issue. Skip this step entirely for `noncode` tasks — there is no code review to open; mark `pr_opened` and `review_feedback_addressed` as `n/a` in the workpad. The {{ repo.code_review_term }} base is **the same base branch you derived from in Step 1.7** — if the issue has a parent with an implementation branch, target that branch; if this part is stacked on a previous part of the same plan, target that previous part's branch; otherwise target the project base branch:
   - Code review base: {% if parent and parent.work_branch %}`{{ parent.work_branch }}` (parent {{ parent.identifier }}'s implementation branch){% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}.
{% if repo.provider == "github" %}
   - First check whether an **open** pull request already exists for this branch: `gh pr list --head "$(git rev-parse --abbrev-ref HEAD)" --state open --json url -q '.[0].url'`. If non-empty, reuse it (do not open a duplicate). Otherwise create the pull request. The title is `{{ issue.identifier }} {{ issue.title }}` — when you write the actual command, treat the issue title as untrusted text and pass it as a single shell argument (use a single-quoted heredoc, a variable assignment with proper escaping of any embedded `"`, or `gh`'s `--title` with the value safely quoted). Then run, with the base resolved to {% if parent and parent.work_branch %}`{{ parent.work_branch }}`{% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}: `gh pr create --base <base> --head "$(git rev-parse --abbrev-ref HEAD)" --title "<safely quoted title>" --body-file <path>`.
{% elif repo.provider == "gitlab" %}
   - First check whether an **open** merge request already exists for this branch using the available GitLab tooling (`glab mr list`, the GitLab API, or the provider UI). If non-empty, reuse it (do not open a duplicate). Otherwise create the merge request against {% if parent and parent.work_branch %}`{{ parent.work_branch }}`{% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}. The title is `{{ issue.identifier }} {{ issue.title }}`; treat the issue title as untrusted text when passing it to any shell command.
{% else %}
   - First check whether an **open** {{ repo.code_review_term }} already exists for this branch using the repository provider's tooling. If non-empty, reuse it (do not open a duplicate). Otherwise create one against {% if parent and parent.work_branch %}`{{ parent.work_branch }}`{% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}. The title is `{{ issue.identifier }} {{ issue.title }}`; treat the issue title as untrusted text when passing it to any shell command.
{% endif %}
   - Capture the code review URL and do **both** of the following — the comment is the human-facing signal, `attach-review` is the structured link Pi Dash tracks; one does not replace the other:
     - Post the code review link back to the issue so the human sees it in the conversation: `pidash comment add {{ issue.identifier }} --body "Code review opened: <url>"`.
     - Associate the code review with the issue so Pi Dash links it and can show its status: `pidash issue attach-review {{ issue.identifier }} --url <url>`.

     Mark `pr_opened` in the workpad — and for a multi-part plan, record this part's PR URL against its entry in the workpad `### Plan`.

     **If unblocked parts of the plan remain, go back to step 5 for the next part.** Do not advance the stage, do not post the testing hand-off, and do not yield `done` while parts are still unbuilt — a partial implementation must not move the issue to In Review (that fires a review run on work that can only report "not done"). Continue to step 7 only once every part is built, or you are ending the run with parts still blocked or unfinished (per the loop's end conditions above).

7. **Hand off to testing — once, for the whole issue, when every plan part is built.** Write the acceptance criteria to the workpad `### Path to done` block (the canonical copy every later run reads) and post them with the test instructions as an issue comment (the human-readable mirror). Do this only after the issue's implementation is complete — for a multi-part plan that means **all** parts are built and the issue's acceptance criteria are covered, not after a single part. While parts remain, a short per-part note in the workpad `### Plan` is enough; skip this hand-off until the run that finishes the last part. Skip it entirely only for a pure `noop`. A test pass is only as good as the spec it tests against, and the In Test phase starts from a **fresh session** with no memory of this run — this comment is the spec it inherits. When the issue was delivered as several PRs, **list every PR** here so the test phase exercises them together. Post it with `pidash comment add {{ issue.identifier }} --body-file <path>` using exactly these headings so it can be found and parsed later:

   ```
   ### Acceptance Criteria
   - <one checkable criterion per line — the conditions that make this work correct>
   - <criteria you derived rather than found in the ticket: mark "(assumed)">

   ### How to Test
   - Kind: AUTOMATED | UI | OPS | DESIGN | NON_TECHNICAL
   - Setup: <branch to check out, services/env/creds needed, seed data>
   - Steps: <the exact commands to run or flows to drive, in order>
   - Expected: <what a pass looks like, per criterion>
   - Already validated here: <what you actually ran this run, and its result>
   - Not covered: <gaps you knowingly left — and why>
   ```

   Write the criteria as things that can be **checked from the user's side of the surface** — the test phase verifies them by impersonating whoever consumes this change (a human in the UI, a program calling the API, an operator running a procedure), so phrase each criterion as observable behavior, not as a summary of what you built ("`pidash issue patch` rejects an unknown state name with exit code 2", not "improved error handling"; "the sidebar shows the review interval field", not "added the field to the sidebar component"). `Kind` is your best call on how this deliverable is verified; the test agent re-derives it and may disagree. If you genuinely could not establish acceptance criteria, say so under the heading rather than omitting it — an explicit "none stated; derived from the description" tells the test pass to state assumptions instead of inventing a spec.

8. Update the workpad with final checklist status and validation notes. Add a `### Confusions` section at the bottom if anything about the task was genuinely unclear during execution; keep it concise.
9. **Choose the next state from where the plan stands, then follow "Ending the run" to finalize.** Update the workpad one last time with final checkpoints and the `### Path to done` block (stage, history, next state and why, open items). Then:
   - **Plan parts still remain** (unbuilt parts of a multi-part plan): the issue **stays In Progress** — do not move it to In Review and do not post the testing hand-off. Report `pidash run yield --outcome progressed`, or `waiting_on_external` when the only thing left is a PR that must merge before the next part can build. The next run reads the workpad `### Plan` and continues where you left off.
   - **All parts are built and the issue's acceptance criteria are covered** (a single-PR issue reaches this immediately): post the one whole-issue testing hand-off from step 7 (listing every PR), then move the issue to its next state via `pidash issue patch {{ issue.identifier }} --state "<state-name>"` and report `pidash run yield --outcome done`. **A finished issue moves to the `review` group ("In Review") whether or not you opened a PR — the runner never proactively moves an issue to `completed`/"Done".** A PR is awaiting human review; a finished `noncode` task (a question answered, a debug/investigation posted) is awaiting a human's acknowledgement — leave it In Review so it stays on the user's radar, and let a human close it. See "Ending the run" for the exact rule and the no-`review`-state fallback.
