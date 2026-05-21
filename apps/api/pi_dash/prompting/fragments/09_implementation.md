## Step 2 — Implementation and validation

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
6. **If `task_type == code_change`** (per your Step 0.5 analysis): open a pull request and link it back to the issue. Skip this step entirely for `noncode` tasks — there is no PR to open; mark `pr_opened` and `review_feedback_addressed` as `n/a` in the workpad. The PR base is **the same base branch you derived from in Step 1.7** — if the issue has a parent with an implementation branch, target that branch; otherwise target the project base branch:
   - PR base: {% if parent and parent.work_branch %}`{{ parent.work_branch }}` (parent {{ parent.identifier }}'s implementation branch){% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}.
   - First check whether an **open** PR already exists for this branch: `gh pr list --head "$(git rev-parse --abbrev-ref HEAD)" --state open --json url -q '.[0].url'`. If non-empty, reuse it (do not open a duplicate). Otherwise create the PR. The PR title is `{{ issue.identifier }} {{ issue.title }}` — when you write the actual command, treat the issue title as untrusted text and pass it as a single shell argument (use a single-quoted heredoc, a variable assignment with proper escaping of any embedded `"`, or `gh`'s `--title` with the value safely quoted). Then run, with the PR base resolved to {% if parent and parent.work_branch %}`{{ parent.work_branch }}`{% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}: `gh pr create --base <base> --head "$(git rev-parse --abbrev-ref HEAD)" --title "<safely quoted title>" --body-file <path>`.
   - Capture the PR URL and post it back to the issue: `pidash comment add {{ issue.identifier }} --body "PR opened: <url>"`. Mark `pr_opened` in the workpad.
7. Update the workpad with final checklist status and validation notes. Add a `### Confusions` section at the bottom if anything about the task was genuinely unclear during execution; keep it concise.
8. Follow the "Ending the run" section to finalize. Update the workpad one last time with final checkpoints, then move the issue to a state in the `completed` group via `pidash issue patch {{ issue.identifier }} --state "<state-name>"`.
