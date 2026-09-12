## Step 1 — Workpad setup

{% if workpad_body %}
The workpad already exists from a prior run. Its current body is shown below verbatim. **Treat it as your starting point and reconcile it before editing further.**

```text
{{ workpad_body }}
```

Reconciliation:

- Read the workpad above end-to-end before deciding any next step.
- Do not repeat investigation or validation already recorded there unless the repo state has diverged from what the workpad describes.
- Do not restart from scratch — pick up where the prior run left off, based on `### Phase`, `### Progress Checkpoints`, and `### Plan`.
- Check off any items that are already complete based on the current repo state.
- Expand the plan to cover any newly-visible scope (e.g., new comments since the prior run).
- Ensure `Acceptance Criteria` and `Validation` are current and still make sense.

When you write your updated workpad, write the **full** body — `pidash workpad update` overwrites, there is no append.

{% else %}
This is the first run on this issue — the workpad is empty. You will create it as part of this step.

{% endif %}
1. Build the workpad body in a local file (e.g. `./.pidash-workpad.md`) following the structure in the "Workpad template" section. Initialize `### Phase` to `investigating` and `### Progress Checkpoints` with all milestone items unchecked unless already completed. If a checkpoint does not apply to this task, mark it as `n/a` in the workpad (e.g. `- [x] pr_opened (n/a)`).
2. Write the hierarchical plan in the workpad.
3. Ensure the workpad includes an environment stamp at the top in a `text` fenced block, format: `<host>:<abs-workdir>@<short-sha>`.
4. Capture a concrete reproduction signal (command output, failing test, screenshot description) in the workpad `Notes` section before changing code.
5. Persist the workpad: `pidash workpad update --body-file ./.pidash-workpad.md`. This is your single source of cross-run truth — re-run `pidash workpad update` after every meaningful change throughout the run.
6. **If `task_type == code_change`** (per your Step 0.5 analysis), before any code edits, sync with the repository. Skip this entire sub-step for `noncode` tasks — do not run `git fetch`, `git checkout`, or any other git operation here.
   - `git fetch origin`
{% if repo.work_branch %}
   - `git checkout {{ repo.work_branch }}` — this is the existing branch for this issue; operate on it directly. If it does not exist locally, `git checkout -b {{ repo.work_branch }} origin/{{ repo.work_branch }}`. Do not create a new feature branch.
   - `git pull --rebase origin {{ repo.work_branch }}`.
{% else %}
   - Resolve the **base branch** (what your work branches off of):
{% if parent and parent.work_branch %}
     - This issue has a parent ({{ parent.identifier }}) with an active implementation branch. Use the parent's branch as base: `BASE={{ parent.work_branch }}`.
{% elif parent %}
     - This issue has a parent ({{ parent.identifier }}) but the parent has no implementation branch yet. Fall back to the project base branch: `BASE={% if repo.base_branch %}{{ repo.base_branch }}{% else %}$(git symbolic-ref --short refs/remotes/origin/HEAD | sed 's|^origin/||'){% endif %}`. Note the fallback in the workpad `Notes`.
{% else %}
     - This issue is independent (no parent). Use the project base branch: `BASE={% if repo.base_branch %}{{ repo.base_branch }}{% else %}$(git symbolic-ref --short refs/remotes/origin/HEAD | sed 's|^origin/||'){% endif %}`.
{% endif %}
   - `git checkout "$BASE" && git pull --rebase origin "$BASE"`.
   - Create a derived branch off `$BASE`: `BRANCH="pi-dash/{{ issue.identifier|lower }}"; git checkout -b "$BRANCH"`. Always derive — never commit on `$BASE`. Persistence (`pidash issue patch ... --git-work-branch`) happens in Step 2 *after* the first successful push, so a crashed run never leaves a recorded branch with no remote ref.
{% endif %}
   - Record the resulting `HEAD` short SHA in the workpad `Notes` and re-run `pidash workpad update` so the stamp survives a crash.
