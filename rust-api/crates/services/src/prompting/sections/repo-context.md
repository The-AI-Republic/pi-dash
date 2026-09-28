---
key: repo-context
title: Repository context
customizable: locked
---
Repository:
{% if repo.url %}
- Remote: {{ repo.url }}
- Base branch: {{ repo.base_branch or "(use the repository's default branch — run `git symbolic-ref refs/remotes/origin/HEAD` to resolve it)" }}
{% if repo.work_branch %}
- Work branch: {{ repo.work_branch }} — check this branch out and commit directly onto it. Do not create a new feature branch.
{% else %}
- Work branch: (none) — create a fresh feature branch off the base branch for your work.
{% endif %}
{% else %}
- Work in the runner's configured working directory. Do not clone or touch any other path.
- This may be an ordinary folder, not a Git repository. Execute the task normally; do not require Git setup, commits, or a PR for non-coding work.
{% endif %}
{% if code_reviews %}

Associated {{ repo.code_review_term }}s (git PRs/MRs already linked to this issue):
{% for cr in code_reviews %}
- {{ cr.title or cr.url }} — {{ cr.url }} (state: {{ cr.state }}{% if cr.merged %}, merged{% endif %}{% if cr.draft %}, draft{% endif %})
{% endfor %}
These are existing code reviews already attached to this issue. Inspect any that are relevant before starting — your task may build on this prior work. When you open a new {{ repo.code_review_term }}, do not duplicate one that is already open here; reuse it instead.
{% endif %}
