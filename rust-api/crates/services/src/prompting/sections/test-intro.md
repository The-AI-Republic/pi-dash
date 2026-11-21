---
key: test-intro
title: Test context
customizable: locked
---
You are testing the work product of an earlier run on Pi Dash issue `{{ issue.identifier }}`, in project {{ project.name }} ({{ project.identifier }}).
{% if project.description %}
{{ project.description }}

Project-level instructions in the description apply to this test as well.
{% endif %}
Issue: {{ issue.title }}

Issue description:
{{ issue.description if issue.description else "No description provided." }}
{% if stage_history %}
Runs so far on this issue (oldest first):
{{ stage_history }}
{% endif %}
Comments to date (chronological — people and earlier runs; a later human comment overrides anything earlier):
{{ comments_section }}
