---
key: software.context
title: Repository context
customizable: locked
---
Repository:
{% if repo.url %}
- Remote: {{ repo.url }}
- Default branch: {{ repo.base_branch or "(resolve with `git symbolic-ref --short refs/remotes/origin/HEAD`)" }}
- This issue's branch: {{ ("`" ~ repo.work_branch ~ "`") if repo.work_branch else "(none yet)" }}
{% else %}
- Work in the runner's configured working directory. Do not clone or touch any other path.
- This may be an ordinary folder, not a Git repository. Git is optional here: do not initialize a repository, demand a repository URL, or require commits or a code review for this task.
{% endif %}
{% if code_reviews %}

This issue's {{ repo.code_review_term }}s (reuse an open one; never open a duplicate):
{% for cr in code_reviews %}
- {{ cr.title or cr.url }} — {{ cr.url }} ({{ "merged" if cr.merged else cr.state }}{% if cr.draft %}, draft{% endif %})
{% endfor %}
{% endif %}
{% if parent and (parent.work_branch or parent.code_reviews) %}

Parent {{ parent.identifier }}{% if parent.work_branch %}, branch `{{ parent.work_branch }}`{% endif %}{% if parent.code_reviews %}, {{ repo.code_review_term }}s:{% endif %}
{% for cr in parent.code_reviews %}
- {{ cr.title or cr.url }} — {{ cr.url }} ({{ "merged" if cr.merged else cr.state }}{% if cr.draft %}, draft{% endif %})
{% endfor %}
{% endif %}
