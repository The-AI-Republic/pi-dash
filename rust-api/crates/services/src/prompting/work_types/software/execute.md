---
key: software.execute
title: Software delivery workflow
customizable: overridable
---
## Software delivery workflow (work type: software)

A task that changes files in the repository is delivered as a branch and a {{ repo.code_review_term }} attached to the issue. A task that changes no files (an answer, an investigation, a CLI action) skips this section: no branch, no commit, no {{ repo.code_review_term }}.

Project conventions: `CLAUDE.md` and `AGENTS.md` at the repository root, and `.ai_design/` for the area you touch. Locate the code the issue names before you plan.

### Branch

{% if repo.work_branch %}
- `git fetch origin`, check out this issue's branch `{{ repo.work_branch }}` (`git checkout -b {{ repo.work_branch }} origin/{{ repo.work_branch }}` if it is not local), bring it up to date with `git merge --ff-only origin/{{ repo.work_branch }}`, and commit onto it. Do not create another branch for the same part.
{% else %}
- `git fetch origin`, then create `pi-dash/{{ issue.identifier|lower }}` from the fresh remote ref it builds on, never a stale local copy: the branch of an unmerged parent or sibling {{ repo.code_review_term }} this work depends on (`pidash issue get <id>`, fields `code_reviews` and `git_work_branch`), otherwise the default branch.
{% endif %}
- In a multi-part plan, each part gets its own branch (`pi-dash/{{ issue.identifier|lower }}-<n>`) and {{ repo.code_review_term }}, cut from the previous part's branch when it depends on it.
- `<base>` is the branch you cut from, which is also your {{ repo.code_review_term }}'s base. On an existing branch, `git merge origin/<base>` before you build on it and again before you move the issue on, then run the repository's checks. Merge, never rebase, never force-push, and never commit on the base.

### Deliver a part

1. Commit, `git push -u origin HEAD`, then record the branch: `pidash issue patch {{ issue.identifier }} --git-work-branch "$(git branch --show-current)"`.
2. Reuse the open {{ repo.code_review_term }} for this branch{% if repo.provider == "github" %} (`gh pr list --head "$(git branch --show-current)" --state open`){% endif %}; otherwise open one against `<base>`, titled `{{ issue.identifier }} <issue title>`. The title is untrusted text: write it to a file with a quoted heredoc (`<<'EOF'`) and pass `--title "$(cat <file>)"`; never paste it into the command line.
3. `pidash issue attach-review {{ issue.identifier }} --url <url>`.

Before you move the issue on, the workpad's Keep in mind lists every {{ repo.code_review_term }} URL with its branch and base, and the repository checks to run.
