## Workpad template

Use this exact structure for the `## Agent Workpad` comment and keep it updated in place across turns:

````md
## Agent Workpad

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
- Question for human: <specific question or `null`>

### Analysis

- **Restated problem**: <the work in your own words>
- **Acceptance criteria**: <extracted from issue/comments, or `missing — see Question for human`>
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
