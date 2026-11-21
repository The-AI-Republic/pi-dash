---
key: software.execute
title: Software delivery workflow
customizable: overridable
---
## Software delivery workflow (work type: software)

A task that changes files in the repository is delivered as a branch and a {{ repo.code_review_term }} attached to the issue. A task that changes no files (an answer, an investigation, a CLI action) skips this section: no branch, no commit, no {{ repo.code_review_term }}.

### Branch

{% if repo.work_branch %}
- `git fetch origin`, then check out this issue's branch `{{ repo.work_branch }}` (`git checkout -b {{ repo.work_branch }} origin/{{ repo.work_branch }}` if it is not local) and commit onto it. Do not create another branch.
{% else %}
- Base: {% if parent and parent.work_branch %}the parent's branch `{{ parent.work_branch }}` while the parent's {{ repo.code_review_term }} (see "Repository context") is unmerged and this issue builds on its code; otherwise {% endif %}the default branch. If this issue builds on another issue's unmerged work (a sibling it depends on), base off that issue's branch (`pidash issue get <id>`, field `git_work_branch`).{% if parent and not parent.work_branch %} If it needs parent code that is not pushed yet, follow "Blocking the run".{% endif %}

- `git fetch origin`, then branch from the fresh remote ref, never a stale local copy: `git checkout -b pi-dash/{{ issue.identifier|lower }} origin/<base>`. In a multi-part plan, a part that depends on the previous one branches from that part's branch.
{% endif %}
- When the branch already exists (a later run, or work sent back from review or test): `git merge origin/<base>` before you build on it and again before you move the issue on, then run the repository's checks. Merge, never rebase, never force-push, and never commit on the base.

### Deliver a part

1. Commit, `git push -u origin HEAD`, then record the branch: `pidash issue patch {{ issue.identifier }} --git-work-branch <branch>`.
2. Reuse the open {{ repo.code_review_term }} for this branch{% if repo.provider == "github" %} (`gh pr list --head <branch> --state open`){% endif %}; otherwise open one against the branch's base, titled `{{ issue.identifier }} {{ issue.title }}` (pass the title as one quoted argument).
3. `pidash issue attach-review {{ issue.identifier }} --url <url>`.

Before you move the issue on, the workpad's Keep in mind lists every {{ repo.code_review_term }} URL with its branch and the repository checks to run.
