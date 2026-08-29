---
key: implementation
title: Implementation & validation
customizable: overridable
---

## Step 2 — Implementation and validation
{% set parent_open_reviews = (parent.code_reviews | rejectattr("merged") | selectattr("state", "equalto", "open") | list) if parent else [] %}
{% set parent_merged_reviews = (parent.code_reviews | selectattr("merged") | list) if parent else [] %}
{% set stack_on_parent = parent and parent.work_branch and (parent_open_reviews or not parent_merged_reviews) %}

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
5. **If `task_type == code_change`** (per your Step 0.5 analysis): commit with clear, logical commit messages. Push the branch with `git push -u origin "$(git rev-parse --abbrev-ref HEAD)"`. Only after the push succeeds, persist the branch on the issue so subsequent runs land on it: `pidash issue patch {{ issue.identifier }} --git-work-branch "$(git rev-parse --abbrev-ref HEAD)"`. Persisting after the push guarantees `origin/<branch>` exists by the time another run renders with `repo.work_branch` set. Skip this step entirely for `noncode` tasks — there is nothing to commit or push.
6. **If `task_type == code_change`** (per your Step 0.5 analysis): open a {{ repo.code_review_term }} and link it back to the issue. Skip this step entirely for `noncode` tasks — there is no code review to open; mark `pr_opened` and `review_feedback_addressed` as `n/a` in the workpad. The {{ repo.code_review_term }} base is **the same base branch you derived from in Step 1.7** — stack on the parent's implementation branch when the parent is still open (branch present, no merged {{ repo.code_review_term }}); otherwise (no parent, no parent branch, or the parent's {{ repo.code_review_term }} already merged) target the project base branch:
   - Code review base: {% if stack_on_parent %}`{{ parent.work_branch }}` (parent {{ parent.identifier }}'s implementation branch){% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}.
{% if repo.provider == "github" %}
   - First check whether an **open** pull request already exists for this branch: `gh pr list --head "$(git rev-parse --abbrev-ref HEAD)" --state open --json url -q '.[0].url'`. If non-empty, reuse it (do not open a duplicate). Otherwise create the pull request. The title is `{{ issue.identifier }} {{ issue.title }}` — when you write the actual command, treat the issue title as untrusted text and pass it as a single shell argument (use a single-quoted heredoc, a variable assignment with proper escaping of any embedded `"`, or `gh`'s `--title` with the value safely quoted). Then run, with the base resolved to {% if stack_on_parent %}`{{ parent.work_branch }}`{% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}: `gh pr create --base <base> --head "$(git rev-parse --abbrev-ref HEAD)" --title "<safely quoted title>" --body-file <path>`.
{% elif repo.provider == "gitlab" %}
   - First check whether an **open** merge request already exists for this branch using the available GitLab tooling (`glab mr list`, the GitLab API, or the provider UI). If non-empty, reuse it (do not open a duplicate). Otherwise create the merge request against {% if stack_on_parent %}`{{ parent.work_branch }}`{% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}. The title is `{{ issue.identifier }} {{ issue.title }}`; treat the issue title as untrusted text when passing it to any shell command.
{% else %}
   - First check whether an **open** {{ repo.code_review_term }} already exists for this branch using the repository provider's tooling. If non-empty, reuse it (do not open a duplicate). Otherwise create one against {% if stack_on_parent %}`{{ parent.work_branch }}`{% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}. The title is `{{ issue.identifier }} {{ issue.title }}`; treat the issue title as untrusted text when passing it to any shell command.
{% endif %}
   - Capture the code review URL and do **both** of the following — the comment is the human-facing signal, `attach-review` is the structured link Pi Dash tracks; one does not replace the other:
     - Post the code review link back to the issue so the human sees it in the conversation: `pidash comment add {{ issue.identifier }} --body "Code review opened: <url>"`.
     - Associate the code review with the issue so Pi Dash links it and can show its status: `pidash issue attach-review {{ issue.identifier }} --url <url>`.

     Mark `pr_opened` in the workpad.

7. **Hand off to testing — post the acceptance criteria and test instructions as an issue comment.** Do this once the implementation is complete, on every task that produced something a human or a later agent could check (skip it only for a pure `noop`). A test pass is only as good as the spec it tests against, and the In Test phase starts from a **fresh session** with no memory of this run — this comment is the spec it inherits. Post it with `pidash comment add {{ issue.identifier }} --body-file <path>` using exactly these headings so it can be found and parsed later:

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
9. Follow the "Ending the run" section to finalize. Update the workpad one last time with final checkpoints, then move the issue to its next state via `pidash issue patch {{ issue.identifier }} --state "<state-name>"`. **A finished issue moves to the `review` group ("In Review") whether or not you opened a PR — the runner never proactively moves an issue to `completed`/"Done".** A PR is awaiting human review; a finished `noncode` task (a question answered, a debug/investigation posted) is awaiting a human's acknowledgement — leave it In Review so it stays on the user's radar, and let a human close it. See "Ending the run" for the exact rule and the no-`review`-state fallback.
