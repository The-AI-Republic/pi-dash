---
key: cloud-project-context
title: Cloud project context
customizable: locked
---
## Scheduled-run context

- Scheduler: {{ scheduler.name }} (`{{ scheduler.slug }}`)
{% if scheduler.description %}- Purpose: {{ scheduler.description }}
{% endif %}- Project: {{ project.name }} ({{ project.identifier }})
{% if project.context %}- Project context: {{ project.context }}
{% endif %}- Run id: {{ run.id }}
