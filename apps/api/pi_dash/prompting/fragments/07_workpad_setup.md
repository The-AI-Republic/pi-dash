## Step 1 — Workpad setup

1. Run `pidash comment list {{ issue.identifier }}` and look for a comment whose body begins with `## Agent Workpad`.
   - If found, record its `id` — you will pass it to `pidash comment update {{ issue.identifier }} <id>` for every subsequent workpad edit.
   - If not found, create one with `pidash comment add {{ issue.identifier }} --body-file <path>` using the structure in the "Workpad template" section. Record the returned `id`.
2. If the workpad existed from a prior run, reconcile it before editing further:
   - Check off any items that are already complete based on the current repo state.
   - Expand the plan to cover any newly-visible scope.
   - Ensure `Acceptance Criteria` and `Validation` are current and still make sense.
3. Set `### Phase` to `investigating` and initialize `### Progress Checkpoints` with all milestone items unchecked unless already completed. If a checkpoint does not apply to this task, mark it as `n/a` in the workpad (e.g. `- [x] pr_opened (n/a)`).
4. Write or update the hierarchical plan in the workpad.
5. Ensure the workpad includes an environment stamp at the top in a `text` fenced block, format: `<host>:<abs-workdir>@<short-sha>`.
6. Capture a concrete reproduction signal (command output, failing test, screenshot description) in the workpad `Notes` section before changing code.
7. Before any code edits, sync with the repository:
   - `git fetch origin`
     {% if repo.work_branch %}
   - `git checkout {{ repo.work_branch }}` — this is an existing branch you must operate on directly. If the branch does not exist locally, `git checkout -b {{ repo.work_branch }} origin/{{ repo.work_branch }}`. Do not create a new feature branch.
   - `git pull --rebase origin {{ repo.work_branch }}`.
     {% else %}
   - Resolve the base branch: `{% if repo.base_branch %}{{ repo.base_branch }}{% else %}$(git symbolic-ref --short refs/remotes/origin/HEAD | sed 's|^origin/||'){% endif %}` — call this `$BASE`.
   - `git pull --rebase origin $BASE` (or equivalent for your branch strategy).
   - Create a fresh feature branch off `$BASE` for your work.
     {% endif %}
   - Record the resulting `HEAD` short SHA in the workpad `Notes`.
