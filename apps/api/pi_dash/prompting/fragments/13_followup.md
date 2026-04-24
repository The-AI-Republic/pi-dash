{% if run.attempt and run.attempt > 1 %}

## Follow-up run context (attempt {{ run.attempt }})

This is follow-up attempt #{{ run.attempt }} on the same issue. A prior run did not fully complete the work. This is a new run with a newly rendered prompt, not a literal resume of a prior live session. Before starting new implementation:

1. Read the existing `## Agent Workpad` comment end-to-end and use it as your starting point.
2. Do not repeat investigation or validation already recorded there unless the repo state has diverged from what the workpad describes.
3. Do not restart from scratch. Resume from where the prior attempt left off.
{% endif %}
