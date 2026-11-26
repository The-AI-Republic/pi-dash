---
key: review-intro
title: Review context
customizable: locked
---
You are reviewing the work product of an earlier run on Pi Dash issue `{{ issue.identifier }}`, in project {{ project.name }} ({{ project.identifier }}).
{% if project.context %}
{{ project.context }}

Project-level instructions in the project context apply to this review as well.
{% endif %}
Issue: {{ issue.title }}
{% if parent %}
Parent issue: {{ parent.identifier }} — {{ parent.title }}. This issue may build on the parent's delivered work (`pidash issue get {{ parent.identifier }}`); judge only what this issue adds.
{% endif %}

Issue description:
{{ issue.description if issue.description else "No description provided." }}
{% if stage_history %}
Runs so far on this issue (oldest first):
{{ stage_history }}
{% endif %}
Comments to date (chronological — people and earlier runs; a later human comment overrides anything earlier):
{{ comments_section }}
