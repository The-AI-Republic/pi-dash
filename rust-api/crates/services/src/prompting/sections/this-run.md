---
key: this-run
title: This run
customizable: locked
---
## This run

{% if run.kind == "scheduler" %}
- Project: `{{ project.identifier }}`; run id: `{{ run.id }}`.
{% else %}
- Issue: `{{ issue.identifier }}`; run id: `{{ run.id }}`; stage: **{{ issue.state }}**.
{% endif %}
- Rendered at: `{{ run.rendered_at }}` (UTC) — the freshness anchor. Everything quoted from Pi Dash below was read then; "What you already have" and "How this run works" say when to re-check.
