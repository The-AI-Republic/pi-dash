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
5. Commit with clear, logical commit messages. Push the branch to the configured remote.
6. Update the workpad with final checklist status and validation notes. Add a `### Confusions` section at the bottom if anything about the task was genuinely unclear during execution; keep it concise.
7. Follow the "Ending the run" section to finalize. Update the workpad one last time with final checkpoints, then move the issue to a state in the `completed` group via `pidash issue patch {{ issue.identifier }} --state "<state-name>"`.
