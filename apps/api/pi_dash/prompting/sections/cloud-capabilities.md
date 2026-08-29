---
key: cloud-capabilities
title: Cloud capabilities
customizable: locked
---
## Capabilities

Available tools:{% for tool in available_tools %}
- `{{ tool }}`{% else %}
- (none){% endfor %}

Unavailable capabilities: {{ unavailable_capabilities | join(", ") }}.
Limits: {{ limits }}.

You do not have a local filesystem, shell, worktree, repository checkout, or Pi Dash CLI. Do not invent or simulate them.
