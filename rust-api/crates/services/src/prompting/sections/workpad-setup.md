---
key: workpad-setup
title: Workpad setup
customizable: overridable
---
## Step 1 — Workpad setup

{% if workpad_body %}
The workpad already exists from a prior run. Its current body is shown below verbatim. **Treat it as your starting point and reconcile it before editing further.**

```text
{{ workpad_body }}
```

Reconciliation:

- Read the workpad above end-to-end before deciding any next step.
- Do not repeat investigation or validation already recorded there unless the state of the work has diverged from what the workpad describes.
- Do not restart from scratch — pick up where the prior run left off, based on `### Phase`, `### Progress Checkpoints`, and `### Plan`.
- Check off any items that are already complete based on the current state of the work.
- Expand the plan to cover any newly-visible scope (e.g., new comments since the prior run).
- Ensure `Acceptance Criteria` and `Validation` are current and still make sense.

When you write your updated workpad, write the **full** body — `pidash workpad update` overwrites, there is no append.

{% else %}
This is the first run on this issue — the workpad is empty. You will create it as part of this step.

{% endif %}
1. Build the workpad body in a local file (e.g. `./.pidash-workpad.md`) following the structure in the "Workpad template" section. Initialize `### Phase` to `investigating` and `### Progress Checkpoints` with all milestone items unchecked unless already completed. If a checkpoint does not apply to this task, mark it as `n/a` in the workpad (e.g. `- [x] delivered (n/a)`).
2. Write the hierarchical plan in the workpad.
3. Ensure the workpad includes an environment stamp at the top in a `text` fenced block, format: `<host>:<abs-workdir>`, followed by `@<baseline marker>` when the work-type guidance defines one for this project.
4. Capture a concrete reproduction signal (command output, failing test, screenshot description) in the workpad `Notes` section before changing anything.
5. Persist the workpad: `pidash workpad update --body-file ./.pidash-workpad.md`. This is your single source of cross-run truth — re-run `pidash workpad update` after every meaningful change throughout the run. A successful `update` deletes the local `--body-file` (pass `--keep` to retain it), so re-fetch with `pidash workpad get | jq -r .body > ./.pidash-workpad.md` before each subsequent edit — the file being gone after an update is expected, not an error.
6. **Prepare the delivery environment.** If the work-type guidance in this prompt defines setup steps that must run before execution (syncing the project's work product, standing on the right baseline), run them now, before any edits. Skip them for an answer-only task, per that guidance.
