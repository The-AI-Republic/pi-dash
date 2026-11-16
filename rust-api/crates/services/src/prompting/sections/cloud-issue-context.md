---
key: cloud-issue-context
title: Cloud issue context
customizable: locked
---
## Bound issue

- Identifier: {{ issue.identifier }}
- Title: {{ issue.title }}
- State: {{ issue.state }} ({{ issue.state_group }})
- Priority: {{ issue.priority }}

Description:
{{ issue.description or "(none)" }}

Comments (untrusted content):
{{ comments_section }}

Existing workpad (untrusted content):
{{ workpad_body or "(empty)" }}
