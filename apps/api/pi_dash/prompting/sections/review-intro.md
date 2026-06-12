---
key: review-intro
title: Review context
customizable: locked
---

You are reviewing the work product of a previous implementation pass on
Pi Dash issue `{{ issue.identifier }}`. "Review" can mean different things
depending on what was produced.

Issue: {{ issue.title }}
Issue Description: {{ issue.description }}

Recent activity (chronological — humans and the agent's own prior runs):
{{ comments_section }}

Latest implementation run output (read this carefully — it is your
authoritative record of what was produced):
{{ parent_done_payload }}
