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

### Path to done

- **Goal**: <one line — what "finished" means for this issue>
- **Stage**: In Progress | In Review | In Test
- **History**: <append one entry per stage move, e.g. `In Progress (3 runs) → In Review (1) → In Progress`>
- **Next state**: <the state you are leaving the issue in — because: <why>>
- **Open items**:
  - [ ] <hand-off list for the next run, in any stage — review findings, failed criteria, unfinished work>
- **Acceptance criteria**: (canonical copy; the hand-off comment mirrors it for humans)
  - [ ] Criterion 1

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
- Waiting on: <`none`, or the open blocker IDs you chose to wait for, and why>

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

- `### Path to done` is shared by every stage — review and test runs edit *only* that block and carry the rest forward; an In Progress run **consumes** `Open items` (checks off / removes what it addressed) and **appends** to `History`, never regenerating the block from scratch or dropping items it did not address.
- The body has **no** outer `## Agent Workpad` heading — that was needed when the workpad was an in-thread comment; it isn't anymore.
- `Waiting on` is a note to your own future self, not an instruction to Pi Dash — nothing on the platform reads it. When you decide to wait for open blockers, write down which ones and *why* (e.g. `Waiting on: PROJ-12 — needs its response shape settled before this endpoint can be written`), call `pidash issue wait {{ issue.identifier }}` so the wait costs no budget, and yield `waiting_on_external`. The next tick shows you those blockers and their states again; the line is there so you remember what you were actually waiting for rather than re-deriving it. Set it back to `none` once you stop waiting.
- `Awaiting human reply` replaces the old `Question for human` field. The actual question text now lives in the comment you posted to the human; the workpad just records that you're waiting on a reply so the next run knows not to re-ask.
