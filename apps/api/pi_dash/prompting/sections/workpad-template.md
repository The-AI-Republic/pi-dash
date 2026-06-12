---
key: workpad-template
title: Workpad template
customizable: locked
---
## Workpad template

Use this exact structure for the workpad body (the document you write via `pidash workpad update --body-file <path>`). Re-write the full body every time — there is no append. The workpad is **not** a comment and is not shown in the issue thread.

````md
```text
<hostname>:<abs-path>@<short-sha>
```

### Phase

- investigating | designing | implementing | validating | opening_pr | addressing_review

### Progress Checkpoints

- [ ] investigation_complete
- [ ] design_choice_recorded
- [ ] implementation_complete
- [ ] validation_complete
- [ ] pr_opened (or `n/a`)
- [ ] review_feedback_addressed (or `n/a`)

### Autonomy / Escalation

- Score: 0
- Type: none
- Safe to continue: true
- Reason: <why this assessment is appropriate>
- Awaiting human reply: <`null`, or a one-line reminder of the question you posted as a comment and when>

### Analysis

- **Restated problem**: <the work in your own words>
- **Acceptance criteria**: <extracted from issue/comments, or `missing — asked in comment`>
- **Proposed approach**: <one or two sentences naming files / areas / components, or actions for noncode>
- **Task type**: code_change | noncode
- **Risks / assumptions**: <material risks, scope assumptions, downstream impact>
- **Decision**: proceed | clarify | split

### Plan

- [ ] 1. Parent task
  - [ ] 1.1 Child task
  - [ ] 1.2 Child task
- [ ] 2. Parent task

### Acceptance Criteria

- [ ] Criterion 1
- [ ] Criterion 2

### Validation

- [ ] targeted test: `<command>`

### Notes

- <short progress note with timestamp>

### Confusions

- <only include if something was confusing during execution>
````

Notes on the structure:

- The body has **no** outer `## Agent Workpad` heading — that was needed when the workpad was an in-thread comment; it isn't anymore.
- `Awaiting human reply` replaces the old `Question for human` field. The actual question text now lives in the comment you posted to the human; the workpad just records that you're waiting on a reply so the next run knows not to re-ask.
