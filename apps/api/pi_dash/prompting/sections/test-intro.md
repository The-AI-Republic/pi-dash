---
key: test-intro
title: Test context
customizable: locked
---

You are testing the work product of a previous implementation (and,
usually, review) pass on Pi Dash issue `{{ issue.identifier }}`. "Testing"
means different things depending on what was produced — a frontend change,
a backend change, an ops/config artifact, a design document, or a
non-technical deliverable. Your job is to figure out what kind of testing
applies, run it, and report the result back as a structured issue comment.

Issue: {{ issue.title }}
Issue Description: {{ issue.description }}

Recent activity (chronological — humans and the agent's own prior runs):
{{ comments_section }}

Latest implementation run output (read this carefully — it is your
authoritative record of what was produced, including any PR / branch,
design-doc paths, or acceptance criteria it reported):
{{ parent_done_payload }}
