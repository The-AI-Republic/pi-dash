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
5. Commit with clear, logical commit messages. Push the branch with `git push -u origin "$(git rev-parse --abbrev-ref HEAD)"`.
6. Open a pull request and link it back to the issue. The PR base is **the same base branch you derived from in Step 1.7** — if the issue has a parent with an implementation branch, target that branch; otherwise target the project base branch:
   - PR base: {% if parent and parent.work_branch %}`{{ parent.work_branch }}` (parent {{ parent.identifier }}'s implementation branch){% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}.
   - First check whether a PR already exists for this branch: `gh pr view --json url,state -q .url 2>/dev/null`. If one exists and is open, reuse its URL (do not open a duplicate). Otherwise: `gh pr create --base <base> --head "$(git rev-parse --abbrev-ref HEAD)" --title "{{ issue.identifier }} {{ issue.title }}" --body <summary referencing {{ issue.identifier }} and the workpad>`.
   - Capture the PR URL and post it back to the issue: `pidash comment add {{ issue.identifier }} --body "PR opened: <url>"`. Mark `pr_opened` in the workpad.
7. Update the workpad with final checklist status and validation notes. Add a `### Confusions` section at the bottom if anything about the task was genuinely unclear during execution; keep it concise.
8. Follow the "Ending the run" section to finalize. Update the workpad one last time with final checkpoints, then move the issue to a state in the `completed` group via `pidash issue patch {{ issue.identifier }} --state "<state-name>"`.
