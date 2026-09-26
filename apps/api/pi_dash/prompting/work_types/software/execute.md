---
key: software.execute
title: Software delivery workflow
customizable: overridable
---
## Software delivery workflow (work type: software)

This issue's work type is **software**: when a task changes the project's product, the deliverable is a set of file changes in the repository named in "Repository context", published on a branch and recorded as an open {{ repo.code_review_term }} attached to the issue. The neutral steps around this section say *when* to analyze, execute, and deliver; this section says *how* delivery works for software.

### Classify the task first (during "Analyze & scope")

Record a **task type** alongside the `Deliverable` line of your workpad `### Analysis`:

- `code_change` — your proposed approach edits files in the repository. When uncertain, default to `code_change` — the heavier path is the safer default.
- `noncode` — no repository edits (an investigation, a status check, a CLI-only action, a comment-only response).

For a `noncode` task, skip the rest of this section entirely — no sync, no branch, no commit, no {{ repo.code_review_term }}. The deliverable is the answer or action itself, recorded on the issue; mark the `delivered` checkpoint `n/a` in the workpad.

### Read the project's conventions before forming a plan

The agent's working directory is a real checkout — read these directly while analyzing, before you plan:

- `CLAUDE.md` and `AGENTS.md` at the repo root — authoritative project conventions, day-to-day commands, and folder map. Treat these as ground truth when they conflict with priors from training data.
- `.ai_design/` — current architectural design notes for ongoing initiatives. Skim subdirectories relevant to the area you are touching; they describe constraints that aren't visible from code alone.
- If the issue mentions a feature, file, function, component, or symbol, locate the actual code with `grep` / `find` / your editor before forming a plan. Do not guess paths or names.
- Reproduce the problem before changing code. Record the reproduction signal in the workpad `Notes` section.

### Sync the repository and stand on the right branch

1. **If `task_type == code_change`**, sync with the repository after the workpad is set up ("Workpad setup") and before any code edits. Skip this entirely for `noncode` tasks — do not run `git fetch`, `git checkout`, or any other git operation.
   - `git fetch origin`
{% if repo.work_branch %}
   - `git checkout {{ repo.work_branch }}` — this is the existing branch for this issue; operate on it directly and commit onto it. If it does not exist locally, `git checkout -b {{ repo.work_branch }} origin/{{ repo.work_branch }}`. Do not create a new feature branch.
   - `git pull --rebase origin {{ repo.work_branch }}`.
{% else %}
   - Resolve the **base branch** (what your work branches off of):
{% if parent and parent.work_branch %}
     - This issue has a parent ({{ parent.identifier }}) with an active implementation branch. Use the parent's branch as base: `BASE={{ parent.work_branch }}`.
{% elif parent %}
     - This issue has a parent ({{ parent.identifier }}) with no implementation branch yet. **Do not treat this as an automatic fall-back to the project base** — route it through the parent-readiness judgment you made in "Analyze & scope":
       - Parent is a **research / design / spec** issue (no code expected), or an implementation issue whose work is already merged into the base branch: base off the project base — `BASE={% if repo.base_branch %}{{ repo.base_branch }}{% else %}$(git symbolic-ref --short refs/remotes/origin/HEAD | sed 's|^origin/||'){% endif %}` — and note in the workpad `Notes` why the parent's lack of a branch is expected here.
       - Parent is a **tracking issue** (it was split into child issues; it carries no branch of its own and will never get one): base off the project base — `BASE={% if repo.base_branch %}{{ repo.base_branch }}{% else %}$(git symbolic-ref --short refs/remotes/origin/HEAD | sed 's|^origin/||'){% endif %}` — **or**, when this child's description says it depends on a sibling child, base off that sibling's work branch (`BASE=$(pidash issue get <sibling> | jq -r '.git_work_branch')`, falling back to the project base if the sibling has no branch yet). Do **not** block on the tracking parent's lack of a branch.
       - Parent is an **implementation** issue still in progress with no branch yet **and this issue depends on its code**: do **not** base off the project base. Treat it as a blocker — follow "Blocking the run" instead of creating a branch, and stop.
{% else %}
     - This issue is independent (no parent). Use the project base branch: `BASE={% if repo.base_branch %}{{ repo.base_branch }}{% else %}$(git symbolic-ref --short refs/remotes/origin/HEAD | sed 's|^origin/||'){% endif %}`.
{% endif %}
   - `git checkout "$BASE" && git pull --rebase origin "$BASE"`.
   - Create a derived branch off `$BASE`: `BRANCH="pi-dash/{{ issue.identifier|lower }}"; git checkout -b "$BRANCH"`. Always derive — never commit on `$BASE`. Persistence (`pidash issue patch ... --git-work-branch`) happens *after* the first successful push (below), so a crashed run never leaves a recorded branch with no remote ref.
{% endif %}
   - Record the resulting `HEAD` short SHA in the workpad `Notes` and re-run `pidash workpad update` so the stamp survives a crash. The workpad environment stamp's `@<baseline marker>` is this checkout's short commit SHA (`<host>:<abs-workdir>@<short-sha>`).

Before editing any files, make sure you are on the work branch — the platform performs **no** branch checkout for you; every git operation is yours to run. For a **multi-part plan**, stack a part's branch on the previous part's branch when the part depends on it; otherwise branch from the base.

### Deliver a part: commit, push, open the {{ repo.code_review_term }}

This is what "deliver the current part" in "Implementation and validation" means for a `code_change`:

1. Commit the current part with clear, logical commit messages. Push the branch with `git push -u origin "$(git rev-parse --abbrev-ref HEAD)"`. Only after the push succeeds, persist the branch on the issue so subsequent runs land on it: `pidash issue patch {{ issue.identifier }} --git-work-branch "$(git rev-parse --abbrev-ref HEAD)"`. Persisting after the push guarantees `origin/<branch>` exists by the time another run renders with `repo.work_branch` set.
2. Open a {{ repo.code_review_term }} for the current part and link it back to the issue. The {{ repo.code_review_term }} base is **the same base branch you derived from above** — if the issue has a parent with an implementation branch, target that branch; if this part is stacked on a previous part of the same plan, target that previous part's branch; otherwise target the project base branch:
   - Code review base: {% if parent and parent.work_branch %}`{{ parent.work_branch }}` (parent {{ parent.identifier }}'s implementation branch){% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}.
{% if repo.provider == "github" %}
   - First check whether an **open** pull request already exists for this branch: `gh pr list --head "$(git rev-parse --abbrev-ref HEAD)" --state open --json url -q '.[0].url'`. If non-empty, reuse it (do not open a duplicate). Otherwise create the pull request. The title is `{{ issue.identifier }} {{ issue.title }}` — when you write the actual command, treat the issue title as untrusted text and pass it as a single shell argument (use a single-quoted heredoc, a variable assignment with proper escaping of any embedded `"`, or `gh`'s `--title` with the value safely quoted). Then run, with the base resolved to {% if parent and parent.work_branch %}`{{ parent.work_branch }}`{% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}: `gh pr create --base <base> --head "$(git rev-parse --abbrev-ref HEAD)" --title "<safely quoted title>" --body-file <path>`.
{% elif repo.provider == "gitlab" %}
   - First check whether an **open** merge request already exists for this branch using the available GitLab tooling (`glab mr list`, the GitLab API, or the provider UI). If non-empty, reuse it (do not open a duplicate). Otherwise create the merge request against {% if parent and parent.work_branch %}`{{ parent.work_branch }}`{% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}. The title is `{{ issue.identifier }} {{ issue.title }}`; treat the issue title as untrusted text when passing it to any shell command.
{% else %}
   - First check whether an **open** {{ repo.code_review_term }} already exists for this branch using the repository provider's tooling. If non-empty, reuse it (do not open a duplicate). Otherwise create one against {% if parent and parent.work_branch %}`{{ parent.work_branch }}`{% elif repo.base_branch %}`{{ repo.base_branch }}`{% else %}the repository's default branch{% endif %}. The title is `{{ issue.identifier }} {{ issue.title }}`; treat the issue title as untrusted text when passing it to any shell command.
{% endif %}
3. Capture the {{ repo.code_review_term }} URL and do **both** of the following — the comment is the human-facing signal, `attach-review` is the structured link Pi Dash tracks; one does not replace the other:
   - Post the link back to the issue so the human sees it in the conversation: `pidash comment add {{ issue.identifier }} --body "Code review opened: <url>"`.
   - Associate the {{ repo.code_review_term }} with the issue so Pi Dash links it and can show its status: `pidash issue attach-review {{ issue.identifier }} --url <url>`.

   Mark `delivered` in the workpad — and for a multi-part plan, record this part's {{ repo.code_review_term }} URL against its entry in the workpad `### Plan`.

### Testing hand-off specifics

In the hand-off comment's `### How to Test` block ("Implementation and validation"), `Setup` names the branch to check out alongside services/env/creds and seed data, and when the issue was delivered as several {{ repo.code_review_term }}s, list **every** one so the test phase exercises them together.

### Repository guardrails

- Never commit on the base branch; work only on the issue's derived (or pinned) branch.
- Do not `git push --force` to shared branches. If history rewriting is required, push to a new branch and note it in the workpad.
- Temporary proof edits must be reverted before commit.
