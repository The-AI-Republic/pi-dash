---
key: review-intro
title: Review context
customizable: locked
---
You are reviewing the work product of an earlier run on Pi Dash issue `{{ issue.identifier }}`, in project {{ project.name }} ({{ project.identifier }}).
{% if project.description %}
{{ project.description }}

Project-level instructions in the description apply to this review as well.
{% endif %}
Issue: {{ issue.title }}

Issue description:
{{ issue.description if issue.description else "No description provided." }}
{% if handoff %}
Hand-off from the In Progress run (what was produced, the acceptance criteria, how to verify, where to look first):

{{ handoff }}
{% endif %}{% if stage_history %}
Runs so far on this issue (oldest first):
{{ stage_history }}
{% endif %}
Comments to date (chronological — people and earlier runs; a later human comment overrides the hand-off):
{{ comments_section }}
