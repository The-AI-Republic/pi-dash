# Unified workpad: one light shared memory for every issue run

**Status:** implemented on branch `pi-dash/unified-workpad` (2026-11-20), against `origin/main` at `c21b46cb0`, which includes NEWPIDASH-320 (#3420) and NEWPIDASH-325 (#3439). §12 lists what is left: the deploy, the measurement and the cleanup.

**Scope:** `rust-api/` (finish endpoint, run creation, workpad write, prompt sections and their tests and fixtures), `runner/` (the `pidash run finish` flag, the CLI reference, the prompt contract test). No database migration. No web UI change. The Django server (`apps/api/`) is out of scope.

**Builds on:** NEWPIDASH-320 (`.ai_design/optimize_pidash_cli_prompt/`, merged). This design uses four things it added: the `.pidash/workpad.md` snapshot, the `if_body_sha256` stale-write guard, `workpad_sha256` stamped into `agent_run.run_config`, and the `.pidash/` git exclude.

**Supersedes,** in `.ai_design/lean_agent_workflow/design.md`:

- D7's "effort scales with the hand-off" (it now scales with what the workpad says was produced);
- D8 (the hand-off, and "the workpad belongs to In Progress");
- D14 (the hand-off's "Next stage" advice; the lifecycle section already gives the routing rules);
- §8.1's `--handoff-file` row, and the `stage_handoff` write in step 2;
- §8.2 and §8.3;
- the hand-off parts of §9.5, §12 (risks "Review and test lose context by not reading the workpad" and "A weak hand-off produces a weak review"), §13 Issue B and Appendix A.5;
- the §14 notes on the fallback hand-off comment, the revalidate "drops the hand-off step" rule and the `pending_stage_handoff` ordering.

Everything else in that design stands: the stage setting, `pidash run finish`, `stage_history`, and the single review and test procedures.

**How to read this:** §3 lists the decisions. §5 is the workpad itself. §6–§8 specify the change. §9 is rollout. §10 is tests. §12 is what is left. Appendix A has the shipped section text. Paths are relative to the repository root; line numbers are at `c21b46cb0`.

## 1. Problem

Each agent run is a fresh session. Think of a production line: before leaving, each worker writes a few lines for the next one about what was done and what to watch out for. That note is the workpad. It sits beside the job ticket (the issue) and the conversation with the customer (the comments), and does not replace them. It is memory, not instructions: what to do comes from each run's own prompt.

Before this change the note was split. `issues.workpad` was written and read by In Progress runs only. Review and test read a separate **hand-off** (`agent_run.done_payload->'stage_handoff'`, also posted as a comment). Review and test findings travelled back as comments. So "what the next run needs to know" went through three channels, picked by which stage writes and which reads.

That caused:

- **Duplication.** The In Progress agent wrote the goal, criteria and status twice.
- **Agent-only notes in comments.** The hand-off and the findings, which are notes for the next agent, sat in the conversation with people.
- **A heavy note.** The In Progress workpad had a fixed three-block template (`Path to done`, `Plan`, `Notes`), and the prompts sent agents to record facts in those blocks from more than twenty places.
- **More machinery.** `--handoff-file`, `stage_handoff`, `pending_stage_handoff`, a bundle query, a comment filter, a gate with a freshness query, and a fallback that posted the hand-off as a second comment.

## 2. Why the hand-off was added, and the answer here

| Reason (lean design §2.3, §8.3)                                                                              | Answer                                                                                                                                              |
| ------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| Review and test prompts carried the workpad and fetched it again, beside the comments and the prior payload. | The workpad is inlined once ("What you already have" already says not to re-fetch).                                                                 |
| Review and test had to rewrite the whole workpad to change one block.                                        | The note is short (§5), and the agent edits `.pidash/workpad.md` in place with its file tools.                                                      |
| The In Progress workpad was the wrong shape for a reviewer.                                                  | The note says where the work is and how to check it, which is what a reviewer needs. There is no implementation-only shape any more.                |
| Do not let work reach review with nothing to read.                                                           | The gate stays, as a simpler rule: an In Progress run must have changed the workpad in this run before it moves the issue to review or test (§8.2). |
| People should see what review and test will read.                                                            | People read the finishing comment, which links what was produced. The note stays between agents.                                                    |

## 3. Decisions

| #   | Decision                                                                                                                                                                                                                                                                                       |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| U1  | **One store.** `issues.workpad` is the only cross-run memory for agents, read and written by every issue run: In Progress, review and test.                                                                                                                                                    |
| U2  | **The hand-off is removed** from prompts, from storage (`stage_handoff`, `pending_stage_handoff`), from the bundle and context, and from the gate. During a compatibility window the server still accepts a `handoff` from an In Progress run and posts it as a comment, as before (§8.1, §9). |
| U3  | **Comments are for people, the workpad is for agents.**                                                                                                                                                                                                                                        |
| U4  | **The workpad is a short note with two parts,** `Keep in mind` and `Log` (§5). It records what is true and what happened, never what the next run should do. No plan checklist and no fixed list of facts.                                                                                     |
| U5  | **The gate checks that the note was written,** not what it says: a coding-task run moving the issue to review or test must have changed the workpad since the run was created.                                                                                                                 |
| U6  | **`stage_history` stays.** It is the platform's record of each run (stage, result, note, date). `Log` is the agent's one line.                                                                                                                                                                 |
| U7  | **A 16,000-character cap** on the workpad as a safety net. The prompt aims well under that.                                                                                                                                                                                                    |
| U8  | **No backfill.** Old `stage_handoff` values stay in `done_payload`, unread. Issues in review or test at deploy have the hand-off as a comment, and the review and test sections say to use it when the workpad does not say where the work is.                                                 |
| U9  | **Scheduler runs are unchanged.** They have no issue and no workpad.                                                                                                                                                                                                                           |
| U10 | **No pasted output in the workpad.** `pidash` JSON pasted for audit goes in a comment, fenced.                                                                                                                                                                                                 |
| U11 | **Multi-part work is recorded as facts,** one bullet per part: `Part 1 of 3 (export): done, <link>` and `Open: part 2 of 3 (import)`.                                                                                                                                                          |

## 4. Non-goals

- Showing the workpad in the web UI.
- Versioning the workpad.
- A section-level update API.
- Any change to `pidash run finish` other than the hand-off removal and the gate in §8.2.

## 5. The workpad

### 5.1 Shape

```md
### Keep in mind

- <only what the next run must not have to rediscover>

### Log

- <YYYY-MM-DD> <stage>: <one line: what this run did>
```

What belongs in **Keep in mind**, one short bullet each:

- where the work product lives (PR URL and branch, document path, record changed) and how to check it;
- what is still open, including review and test findings, as `Open: …`;
- the parts of multi-part work, as facts (U11);
- decisions and why, when the next run might otherwise undo them;
- traps: dead ends, environment quirks, the command that actually works.

What does not belong: instructions or advice for the next run (its own prompt says what to do), anything already in the issue or the comments, step-by-step progress, and pasted output.

Write facts, not directions: `Open: Windows path skips the pin`, not `Fix the Windows path next`.

### 5.2 Rules for every run

1. Read it first. Do not redo what it says is done.
2. If the run did something: update **Keep in mind** (add what the next run needs; change or remove bullets that are no longer true) and append one **Log** line, keeping the last ten. A run that only finds nothing changed adds nothing, so idle ticks do not push real Log lines out.
3. Keep it to about 40 lines. The server refuses more than 16,000 characters.

### 5.3 By stage

| Run                   | Keep in mind                                            | Comment (for people)                                  |
| --------------------- | ------------------------------------------------------- | ----------------------------------------------------- |
| In Progress moving on | says where the work is and how to check it              | one or two lines: what was produced, with the link    |
| In Progress staying   | what is still open and any traps                        | none unless a person needs to know something          |
| Review                | each confirmed finding as `Open: …`                     | the verdict, what was checked and what is wrong       |
| Test                  | each failure as `Open: …`, with the check that shows it | pass or fail per criterion, one line of evidence each |
| Blocked               | what the run is waiting for                             | what the person must do                               |

## 6. Prompts

Paths are under `rust-api/crates/services/src/prompting/`.

### 6.1 Recipes

`recipes.rs` `RECIPES`: `S("workpad")` is inserted into the review and test recipes after `S("lifecycle")`.

```text
KIND_CODING_TASK: this-run, intro, SLOT_CONTEXT, relationships, run-framing, pidash-cli, lifecycle,
                  default-posture, autonomy, analyze-and-scope, workpad, SLOT_EXECUTE, implementation, blocking   (unchanged)
KIND_REVIEW:      this-run, review-intro, SLOT_CONTEXT, run-framing, pidash-cli, lifecycle, workpad, review, SLOT_REVIEW, blocking
KIND_TEST:        this-run, test-intro, SLOT_CONTEXT, run-framing, pidash-cli, lifecycle, workpad, test, SLOT_TEST, blocking
KIND_SCHEDULER:   unchanged
```

The doc comment above `RECIPES` says every issue run reads and writes the workpad.

### 6.2 Section changes

Every change below is in the branch. "Keep in mind" always appears as the literal phrase "the workpad's Keep in mind".

| File                                                 | Line(s) at `c21b46cb0` | Change                                                                                                                                                                                                                                                   |
| ---------------------------------------------------- | ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sections/workpad.md`                                | whole file             | Appendix A.1. The heading is `## {% if run.kind == "coding-task" %}Step 1 — {% endif %}Workpad`, so coding-task keeps its numbering and review/test get `## Workpad`.                                                                                    |
| `sections/pidash-cli.md`                             | 23                     | The kind branch ("and your workpad" / "and the hand-off") → " and the workpad" for every issue run.                                                                                                                                                      |
| `sections/pidash-cli.md`                             | 30-32                  | The workpad command line loses its coding-task gate. Its reference reads `See "{% if run.kind == "coding-task" %}Step 1 — {% endif %}Workpad"`, which matches the heading for each kind (contract 5 of `runner/src/cli/prompt_contract.rs`).             |
| `sections/pidash-cli.md`                             | 71                     | `Check intent against the issue{% if run.kind != "scheduler" %}, its comments and the workpad{% else %} and its comments{% endif %}`. The scheduler render is unchanged.                                                                                 |
| `sections/pidash-cli.md`                             | 72                     | Pasted JSON goes "into a comment" for every kind (U10).                                                                                                                                                                                                  |
| `sections/lifecycle.md`                              | 9                      | ", the workpad" is no longer coding-task only.                                                                                                                                                                                                           |
| `sections/lifecycle.md`                              | 31                     | The `--handoff-file .pidash/handoff.md` branch is removed.                                                                                                                                                                                               |
| `sections/lifecycle.md`                              | 38                     | The fallback's "and then the hand-off as a second comment" branch is removed.                                                                                                                                                                            |
| `sections/run-framing.md`                            | 14                     | "from the issue and its comments" → "from the issue, its comments and the workpad".                                                                                                                                                                      |
| `sections/review-intro.md`, `sections/test-intro.md` | 16-20, 24              | The text from `{% if handoff %}` up to and including the `{% endif %}` before `{% if stage_history %}` is removed; `{% if stage_history %}` stays. Line 24 ends "a later human comment overrides anything earlier".                                      |
| `sections/review.md`                                 | 11, 14, 17, 24         | Appendix A.2. Line 14 loses `Give the "review focus" items the most attention.` and gains nothing: the note does not steer the reviewer.                                                                                                                 |
| `sections/test.md`                                   | 11, 17, 24             | Appendix A.3.                                                                                                                                                                                                                                            |
| `sections/implementation.md`                         | 10, 12, 14, 17-30, 34  | Appendix A.4.                                                                                                                                                                                                                                            |
| `sections/analyze-and-scope.md`                      | 10                     | "Once a plan is in the workpad, or a person approved one" → "Once the parts are noted in the workpad, or a person approved a plan".                                                                                                                      |
| `sections/analyze-and-scope.md`                      | 14                     | "and record what matters in the workpad" → "and note in the workpad's Keep in mind only what a later run would otherwise have to re-read".                                                                                                               |
| `sections/analyze-and-scope.md`                      | 21                     | Step 4 keeps the analysis in the session; only "the decisions and assumptions a later run could not see from the issue and comments" go in the workpad's Keep in mind.                                                                                   |
| `sections/analyze-and-scope.md`                      | 26                     | The parts are noted as facts (U11), with the example `Part 1 of 3 (export): done, <link>`. (No "PR": the core-section neutrality test refuses software wording.)                                                                                         |
| `sections/autonomy.md`                               | 11                     | "in the workpad `Notes` and the hand-off" → "in the workpad's Keep in mind".                                                                                                                                                                             |
| `sections/default-posture.md`                        | 9                      | "Do not use it to track your own progress; that is the workpad's job." → "Do not use it for notes to the next run; those go in the workpad."                                                                                                             |
| `sections/default-posture.md`                        | 10, 12                 | "workpad `Notes`" → "the workpad's Keep in mind".                                                                                                                                                                                                        |
| `sections/blocking.md`                               | 10-12                  | Step 2 ("Note in the workpad's Keep in mind what you are waiting on.") is no longer coding-task only; step 3 is always 3.                                                                                                                                |
| `sections/relationships.md`                          | 56                     | Findings are folded "into your work" (not the workpad); only "the facts a continuation run would otherwise re-fetch" go in the workpad's Keep in mind.                                                                                                   |
| `sections/relationships.md`                          | 59-60                  | "record … in your workpad" → "note … in your workpad's Keep in mind". Line 58's `pidash workpad get <ID>` (reading a blocker's workpad) stays.                                                                                                           |
| `sections/relationships.md`                          | 62                     | "no one reads your workpad" → "the platform does not read your workpad".                                                                                                                                                                                 |
| `work_types/software/execute.md`                     | 12, 40                 | "the workpad `Notes`" → "the workpad's Keep in mind".                                                                                                                                                                                                    |
| `work_types/software/execute.md`                     | 49                     | "Record the resulting `HEAD` short SHA and the base" → "Note the branch and its base" (a SHA goes stale on every commit).                                                                                                                                |
| `work_types/software/execute.md`                     | 71                     | The part's URL is noted against the part (U11).                                                                                                                                                                                                          |
| `work_types/software/execute.md`                     | 83-85                  | "Hand-off specifics" → "What the next run needs": before moving on, the workpad's Keep in mind lists every {{ repo.code_review_term }} (URL and branch) and the checks and flows to run.                                                                 |
| `work_types/software/execute.md`                     | 90                     | "note it in the workpad" → "note it in the workpad's Keep in mind".                                                                                                                                                                                      |
| `work_types/software/review.md`                      | 8                      | "the hand-off links it" → "the workpad links it".                                                                                                                                                                                                        |
| `work_types/general/execute.md`                      | 24, 26                 | The deliverable's link is noted against the part (U11); the "In the hand-off, **Produced** …" sentence becomes "Before you move on, the workpad's Keep in mind says where each deliverable lives, how to open it, and how its reader or user checks it." |
| `work_types/general/review.md`                       | 8, 11                  | "the hand-off" → "the workpad".                                                                                                                                                                                                                          |

Unchanged: `this-run.md`, `intro.md`, `guardrails.md` and `session-framing.md` (scheduler only), `relationships.md:58`.

No new text names the state "In Review" or "In Test", so `composer.rs` `composed_prompts_name_exactly_the_enabled_stages` passes with a stage switched off.

### 6.3 Context and preview

- `context.rs:555` `comments_section(comments, handoff)` → `comments_section(comments)`. The hand-off filter and `strip_handoff` (`:599`) are removed with their test.
- `IssueContextInput.handoff` is removed. `build_context` still emits `"handoff": ""` until cleanup: rendering is strict-undefined, so an override that still names `handoff` keeps rendering.
- `rust-api/crates/api/src/prompting/mod.rs:2233-2241`: the preview no longer reads a hand-off.
- `workpad_body` was already in the context for every kind (`creation.rs:2390`).

### 6.4 Overrides

`prompting/seed.rs`:

- `LEAN_WORKFLOW_REWRITTEN_KEYS` (`:444`): `workpad`, `review` and `test` are added. The other changed sections are already listed or are `locked` (`pidash-cli`, `lifecycle`, `run-framing`, `review-intro`, `test-intro`, `blocking`, `relationships`).
- `LEAN_WORKFLOW_RETIRED_MARKERS` (`:466`) gains `--handoff-file`, `{{ handoff }}`, `{% if handoff`, `### Hand-off`, `### Path to done`, `` workpad `Plan` ``, `` workpad `Notes` ``, `not read by review or test`, `Pick up from its open items and plan`.
- `LEAN_WORKFLOW_REQUIRED_STEPS` (`:501`): the `implementation` marker changes from the hand-off words to `Keep in mind`, so an old override ("Update the workpad after each meaningful step") is flagged until it is rewritten.
- Flagging stays report-only: a flagged override still wins. The gate (§8.2) does not depend on wording, so a stale override cannot make moves impossible. A stale `workpad` override does now render into review and test runs as well; it is flagged, and `revalidate_section_overrides` must be run after the deploy (§12).
- `ops/prompting.rs` test helper `known_key` now excludes only keys with a required step, since every unlocked recipe section is in the rewritten list.

## 7. Runner

### 7.1 `pidash run finish`

`runner/src/cli/run_cmd.rs`: `--handoff-file` is `hide = true`. When given, it prints `note: --handoff-file is deprecated: write what the next run needs in the workpad` to stderr and **still reads the file and sends `handoff`** during the compatibility window. That keeps a prompt rendered before the change working against either server, since runner releases are not tied to server deploys (`MIN_RUNNER_VERSION` only shows a banner).

`runner/docs/cli-reference.md` (served by `pidash docs`):

- line 28: pasted JSON goes "into a comment" (U10);
- lines 80, 84, 104: no "use the workpad" for sub-steps, issue identifiers or progress tracking; a new issue's identifier goes in the comment;
- line 125 (`issue wait`): "Note what you are waiting on in the workpad's Keep in mind first";
- line 162 (Comment): "Notes for the next agent run go in the workpad, not in comments.";
- lines 198-201, 236 (Workpad): the shared-memory description;
- lines 470-472 (`run finish`): `--handoff-file` and "stores the hand-off" removed;
- line 545: "Update the workpad when something happens the next run must not lose."

`runner/src/cli/docs.rs:264` keeps `"handoff-file"` in its placeholder list while the hidden flag exists.

`runner/src/cli/workpad.rs`: the module doc describes the shared memory.

### 7.2 Prompt contract test

`runner/src/cli/prompt_contract.rs`: contract 2 (`handoff_file_is_coding_task_only`, `:674`) is now `no_render_mentions_handoff_file`. Every issue render still has the run-finish syntax line, and no render mentions `handoff-file`.

## 8. Server

### 8.1 Finish endpoint

`rust-api/crates/api/src/v1_work_items/handlers_finish.rs`:

**Removed:**

- `check_handoff` (`:364`), `current_handoff_exists` (`:1009`) and their tests;
- the `pending_stage_handoff` write (`:608-614`) and the `stage_handoff` write (`:773-779`);
- `PENDING_HANDOFF_KEY` (`:82`);
- the refusal of a hand-off from a review or test run. Such a hand-off is now dropped (`req.handoff = None`), so it is neither refused nor posted.

The module doc lists five steps: validate (with the workpad gate), move, post the comment, post a legacy hand-off comment, write the result.

**Kept until cleanup** (§12): `parse_finish_body`'s `handoff` parsing and its checks (20k cap, empty, identical to the comment), `FinishPlan.handoff_comment`, the post of an In Progress run's hand-off as a comment on a move to review or test, and `HANDOFF_MAX_CHARS`. A `handoff` in the request is logged at `info` with the run id and phase kind.

**Kept:** `STAGE_HANDOFF_KEY` as a documented constant, so readers know the key exists in old `done_payload` rows.

`rust-api/crates/services/src/runner_runs/finalization.rs`: `FINISH_KEPT_KEYS` is deleted. `merge_done_payload` keeps the yield keys only, as Django does. Its test asserts that an old `stage_handoff` does not survive completion.

### 8.2 The gate

`check_workpad_written(stamped_sha: Option<&str>, current_sha: &str) -> Result<(), Refusal>` replaces `check_handoff` in step 1 of `finish_inner`, before any write. The caller decides when it applies (below).

- **Phase kind:** a project-move replacement run stamps no `phase_kind` but renders the prompt for the issue's current state, which can be review or test. An empty kind is therefore read from the state the run is finishing in (`template_name_for`), not assumed to be coding-task.
- **When:** coding-task, the target differs from the current state (`current_state_id != Some(target.id)`), `target.is_review_or_test()`, and no legacy `handoff`. A retry after a successful move finds the issue already in the target and is not gated.
- **Check:** `fetch_workpad_hashes` reads `agent_run.run_config` (sea-orm `select_only`; `fetch_yield_run` does not load it) and `issues.workpad`. It hashes the workpad with `pidash_services::orchestration::creation::workpad_sha256`. The move is refused when the pass's start hash equals the current hash. The start hash is `run_config.stage_start_workpad_sha256` when present, else `run_config.workpad_sha256`. A run with no stamped hash (created before NEWPIDASH-320) is not gated.
- **Continuations:** the gate measures from the start of the In Progress pass, not from the latest run. `create_continuation_run` copies the parent's start hash (or its `workpad_sha256`) into `stage_start_workpad_sha256` when parent and child are both coding-task runs (`continued_stage_start_sha`). So a note written by an earlier run of the pass counts. The key is server-only: the runner envelope never copies it, and `workpad_sha256` stays the snapshot's own hash for the stale-write guard. A project-move replacement strips it with the other snapshot keys.
- **Refusal:** 400 `{"error": "update the workpad before moving to review or test: note where the work is and how to check it"}` (`WORKPAD_UNCHANGED_MESSAGE`).

After review sends work back, the next In Progress run starts from the reviewer's note and must change it before moving on again. That replaces the old freshness query.

The `issue patch --state` fallback is not gated, as before.

### 8.3 Bundle

`rust-api/crates/services/src/orchestration/creation.rs`: `BUNDLE_HANDOFF_SQL`, `orm::bundle_handoff_select`, `StageInputs.handoff` and its SQL pin test are removed. `load_stage_inputs` loses its `phase_kind` parameter, which only gated the hand-off query. Its four callers are updated, and their now-unused `kind` locals are removed. `stage_history` stays.

### 8.4 Snapshot for every issue run

`creation.rs`: a new `carries_workpad(kind)` (true for coding-task, review and test) gates the three `workpad_snapshot` sites. `stamp_run_context_covers_kinds` asserts it. The runner already writes `.pidash/workpad.md` for any assign carrying `workpad_body` (`runner/src/daemon/supervisor.rs`), so no runner change is needed.

`creation.rs` `LEAN_REVIEW_SECTIONS` is 10 (it was 9).

### 8.5 Size cap

`rust-api/crates/services/src/operations/workpad.rs`: `WORKPAD_MAX_CHARS = 16_000` and `check_workpad_size`, called in `update` after `validate_workpad_write` and before the write. Over the cap it returns `Error::FieldErrors(WORKPAD_TOO_LONG)`, a 400 `{"error": "workpad is longer than 16000 characters: shorten it (drop Keep in mind bullets that are no longer true and keep the last ten Log lines) and retry"}`. It counts characters, not bytes. The size check runs before the stale-write check, so an oversize stale write gets 400, not 412. The v1 workpad PATCH is the only live writer. The cap applies only to agent writes: requests carrying `X-Pi-Dash-Run-Id`, which the CLI sends on every mutating call from inside a run (`WorkpadWrite.from_run`). A person's edit in the UI is never refused for length. Existing longer workpads are not truncated; the next agent write must fit.

## 9. Rollout and compatibility

### 9.1 Order

1. Server and prompts deploy. The runner change can ship in the same or a later runner release; neither order breaks a move to review.
2. Run `revalidate_section_overrides` and the measurement (§10.1).
3. Cleanup (§12) when all of these hold: the deploy has been live for 14 days; `MIN_RUNNER_VERSION` is at least the runner tag containing §7.1; and no active override contains `handoff`, which strict undefined would otherwise turn into a render error once the context key goes.

### 9.2 Issues in flight at deploy

- **In review or test:** the workpad may be in the old layout, with no link to the work. The review and test sections say to use the hand-off an earlier run posted in the comments (Appendix A.2, A.3).
- **In Progress, prompt rendered before the deploy:** it passes `--handoff-file`. Both old and new CLIs send it, the gate is satisfied, the hand-off is posted as a comment as before, and review reads it there.
- **In Progress, new prompt, old-layout workpad:** the workpad section says to rewrite it in the new shape on the first update, keeping only what is still true.

### 9.3 Old runners

| Runner                | What happens                                                                                                                                                                                                                                                                      |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Without NEWPIDASH-320 | No snapshot file and no `PIDASH_ISSUE_IDENTIFIER`. `pidash-cli.md` already says to pass `{{ issue.identifier }}` when a bare command says the identifier is missing, and to fetch the file with `jq` when `--to-file` is unknown. No hash guard: a concurrent write is last-wins. |
| Without `finish`      | The lifecycle fallback (comment, `issue patch --state`, `run yield`), minus the hand-off comment. Not gated (unchanged).                                                                                                                                                          |

## 10. Tests

All are in the branch. "Run" means it ran green locally.

| Test                                                                                                                                                                                                                                                                                                            | Where                                                                                                           | Run                      |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- | ------------------------ |
| Every issue-run prompt includes the workpad section and `pidash workpad update`; no template contains `--handoff-file`, `{{ handoff }}`, `### Hand-off` or `### Path to done`; "hand-off" appears only in the in-flight sentence of review and test. Replaces the "review and test have no workpad" assertions. | `prompting/composer.rs` `issue_run_prompts_drop_the_retired_end_of_run_commands`                                | yes                      |
| The lifecycle fallback names no hand-off for any kind.                                                                                                                                                                                                                                                          | `composer.rs` `old_runner_fallback_stops_the_clock`                                                             | yes                      |
| Coding-task: `## Step 1 — Workpad`, `### Step 7 — Leave the note`, the gate sentence, no `--handoff-file`.                                                                                                                                                                                                      | `composer.rs` `coding_task_leaves_the_note`                                                                     | yes                      |
| Review/test: `## Workpad`, the workpad body, the run history, no hand-off block, no gate sentence; an empty workpad renders "There is no workpad yet."                                                                                                                                                          | `composer.rs` `review_and_test_read_the_workpad_and_history`                                                    | yes                      |
| "write no comment" wording.                                                                                                                                                                                                                                                                                     | `composer.rs` `review_and_test_keep_the_behaviour_changing_rules`                                               | yes                      |
| Stage-name and neutrality tests, size budgets (`SIZE_BUDGETS`, `WHOLE_PROMPT_BUDGETS`, `CLI_SECTION_BUDGET`).                                                                                                                                                                                                   | `composer.rs`, `neutrality.rs`                                                                                  | yes                      |
| `review` and `test` render clean under an empty context.                                                                                                                                                                                                                                                        | `renderer.rs` `EMPTY_CONTEXT_CLEAN`                                                                             | yes                      |
| Samples: an agent comment instead of the hand-off comment; a §5.1-shaped `workpad_body`; `"handoff": ""`.                                                                                                                                                                                                       | `prompting/validation.rs`                                                                                       | yes                      |
| Rendered fixtures (`UPDATE_PROMPT_FIXTURES=1`); `FIX-recipes.json` (review/test recipes and expansions, lengths 10) and `FIX-registry.json` (`body_len`, `body_sha256` and `overridable_default_bodies` of the changed sections) hand-edited to match.                                                          | `rust-api/fixtures/prompting/`                                                                                  | yes                      |
| Override markers: a review override teaching `--handoff-file` is flagged; an `implementation` override without "Keep in mind" is flagged.                                                                                                                                                                       | `ops/prompting.rs`                                                                                              | yes                      |
| Gate: refused when unchanged; allowed after a change and with no stamped hash. A continuation carries the pass start hash only between In Progress runs.                                                                                                                                                        | `handlers_finish.rs` `workpad_gate`; `creation.rs` `continuation_carries_the_pass_start_workpad_hash`           | yes                      |
| The pinned refusal: `finish_missing_handoff` → `finish_workpad_unchanged`.                                                                                                                                                                                                                                      | `rust-api/fixtures/v1_work_items/handlers/F18-11.work_items.json`, `handlers_finish.rs` `replay_finish_fixture` | yes                      |
| `merge_done_payload` keeps the yield keys and no `stage_handoff`.                                                                                                                                                                                                                                               | `finalization.rs`                                                                                               | yes                      |
| `carries_workpad` for each kind.                                                                                                                                                                                                                                                                                | `creation.rs` `stamp_run_context_covers_kinds`                                                                  | yes                      |
| Size cap counts characters.                                                                                                                                                                                                                                                                                     | `operations/workpad.rs` `workpad_size_cap_counts_characters`                                                    | yes                      |
| No render mentions `handoff-file`.                                                                                                                                                                                                                                                                              | `runner/src/cli/prompt_contract.rs`                                                                             | yes                      |
| End to end: refused finish with an unchanged workpad (nothing moves, nothing posted), then 200 after a workpad update; a legacy hand-off is posted, dedupes on retry, and no `stage_handoff` survives completion.                                                                                               | `rust-api/contract-tests/runner/test_finish_workpad.py` (was `test_finish_handoff.py`)                          | needs the contract stack |
| Agent PATCH (run header) over 16,000 characters → 400 on Rust; at the cap in multi-byte characters → 200; a PATCH without the run header is not capped.                                                                                                                                                         | `rust-api/contract-tests/orchestration/test_workpad.py`                                                         | needs the contract stack |

Measured prompt sizes (rendered fixtures, bytes): coding-task 32,451 (was 32,142), review 14,292 (was 12,324), test 15,359 (was 13,308). All are within `WHOLE_PROMPT_BUDGETS` (56,000 / 18,000 / 19,000), and without the CLI section within `SIZE_BUDGETS` (50,000 / 12,000 / 13,000).

### 10.1 Measurement

On testpidash, in a project with review on, after the deploy:

1. Create three small code issues. For each, open a PR by hand with one planted defect (an off-by-one, a missing error path, a wrong default), write a workpad that says where the PR is and how to run its tests, and move the issue to In Review.
2. For each review run, record whether it named the defect and sent the work back, the review prompt size, and the run's token usage.

Pass: review names at least two of the three defects and sends them back. Three runs show gross regressions, not small differences; a finer comparison needs more trials.

## 11. Risks

| Risk                                                                                              | Mitigation                                                                                                                                                                                                |
| ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The note grows into the old heavy workpad.                                                        | Two parts, about 40 lines, ten Log lines, nothing added by idle runs, the 16,000-character cap, the rule to remove bullets that are no longer true, and the prompt sections keep analysis in the session. |
| The note starts instructing the next run.                                                         | The workpad section, the CLI reference and §5.1 all say "facts, not directions", with an example. Review no longer reads a "review focus" from the note.                                                  |
| A review or test run damages what In Progress wrote.                                              | The note is short and edited in place. The NEWPIDASH-320 hash guard refuses a stale write with 412.                                                                                                       |
| Two runs overlap: `finish` creates the next stage's entry run while the old agent is still alive. | The entry run's hash is stamped at creation. A late write by the old agent can only make the gate pass, not fail, and the new run's first stale write gets 412.                                           |
| A stale `workpad` override renders into review and test.                                          | It is flagged by `revalidate_section_overrides` (§6.4), which is part of the deploy steps.                                                                                                                |
| The reviewer is swayed by the implementer's notes.                                                | The review section says to judge the work itself and use the note only to find it. Checked in §10.1.                                                                                                      |
| The gate is satisfied by a trivial edit.                                                          | Accepted. It stops the common failure (moving on without a word); the prompt says what the note must contain.                                                                                             |
| The gate is satisfied by an edit the run did not make (a person's edit during the pass).          | Known limitation, accepted. A hash cannot tell who changed the workpad. Attributing writes to runs would need a write log; the gate is a safety net, not proof the run wrote its note.                    |
| People lose the hand-off comment on a move to review.                                             | The In Progress finishing comment links what was produced (§5.3).                                                                                                                                         |

## 12. What is left

1. **Review and merge** the branch. Then run the two contract tests marked "needs the contract stack" in CI.
2. **Deploy**, then run `pidash-api revalidate_section_overrides` and the measurement (§10.1). Pass: §10.1's criterion. Rollback: redeploy the previous server image. No data was migrated, and the hand-off path is still accepted during the window, so both directions are safe.
3. **Cleanup,** when §9.1 step 3 holds. Remove:
   - `handoff` parsing in `parse_finish_body` (a `handoff` key then gets 400 `the hand-off was removed: write what the next run needs in the workpad`);
   - `FinishPlan.handoff_comment` and its post, `HANDOFF_MAX_CHARS`, and the legacy exemption in the gate;
   - the empty `handoff` context key;
   - the hidden CLI flag and the `docs.rs` placeholder;
   - the in-flight sentence in `review.md` and `test.md` (and the composer assertion that allows it).

   Acceptance: `finish` with `handoff` answers 400 with that message, and no template, context or CLI path mentions `handoff`.

## Appendix A. Shipped section text

### A.1 `sections/workpad.md`

````md
---
key: workpad
title: Workpad
customizable: overridable
---

## {% if run.kind == "coding-task" %}Step 1 — {% endif %}Workpad

The workpad is the shared memory of the agent runs on this issue: a short note of what happened and what is true, like a shift note on a production line. It is memory, not instructions. Every run on this issue starts from it. It is for agents only and never shown in the comments: write what the next run needs and a person does not. The issue and comments are already in front of the next run, so do not copy them.

{% if workpad_body %}
The workpad so far:

```text
{{ workpad_body }}
```

Read it first, and do not redo what it says is done. If it uses an older layout, rewrite it in the shape below on your first update, keeping only what is still true.
{% else %}
There is no workpad yet. Start one in this shape.
{% endif %}

```md
### Keep in mind

- <only what the next run must not have to rediscover>

### Log

- <YYYY-MM-DD> <stage>: <one line: what this run did>
```

**Keep in mind** holds, one short bullet each: where the work is and how to check it; what is still open, as `Open: …`; decisions the next run might otherwise undo; traps and the commands that actually work. Write facts, never instructions for the next run: its own prompt tells it what to do. Change or remove bullets that are no longer true.

Each run that does something: update **Keep in mind** and add one **Log** line, keeping the last ten. A run that only finds nothing changed adds nothing.{% if run.kind == "coding-task" %} A move to review or test is refused until this run has updated the workpad.{% endif %} Keep it to about 40 lines, with no pasted output.

Your workpad is in `.pidash/workpad.md` (also shown above). Edit it, then run `pidash workpad update`; the file is always the full body. If the file is missing, run `pidash workpad get --to-file` first. If an update is rejected as changed, run `pidash workpad get --to-file` and re-apply your edit.
````

### A.2 `sections/review.md`, changed lines

```md
The workpad above says where the work product is and how to check it. Find it there, then judge the work product itself, not the note about it. If the workpad does not say where the work is, an earlier run may have posted a hand-off in the comments: use that.

2. Look for what is wrong or missing against the acceptance criteria.

3. Add each confirmed finding to the workpad's Keep in mind as `Open: …`, then write one comment for people: the verdict, what you checked, and what is wrong. If nothing has changed since your last pass, write no comment.

- **Defects** — findings you could not fix. Move back to In Progress; the workpad's `Open:` bullets are the list.
```

### A.3 `sections/test.md`, changed lines

```md
You are the first user of this work, not its author or its reviewer. The workpad says where the work is and how to check it. If it does not, an earlier run may have posted a hand-off in the comments: use that, or derive the criteria from the issue and note them in the workpad.

5. Add each failure to the workpad's Keep in mind as `Open: …`, with the check that shows it. Then write one comment for people: pass or fail per criterion, one line of evidence each. If nothing has changed since your last pass, write no comment.

- **Defects** — a criterion fails. Move back to In Progress; the workpad's `Open:` bullets are the list.
```

### A.4 `sections/implementation.md`, changed lines

```md
1. Implement against the plan. Update the workpad when something happens the next run must not lose.
2. When the task needs a non-trivial choice, note the choice and why in the workpad's Keep in mind.
3. **Deliver the current part** per the work-type guidance: produce or publish it, record it on the issue, and put its link in the workpad's Keep in mind. For an answer-only task the answer, posted in your finishing comment, is the deliverable.

### Step 7 — Leave the note

Before you move the issue on, the workpad's Keep in mind must say where the work is (every link or path) and how to check it. A move to review or test is refused until this run has updated the workpad. {% if stages.final_state %}For a task that ends at {{ stages.final_state }}{% else %}For a task that needs no later stage{% endif %}, the answer goes in your comment.

Your comment for people, when you move the issue on: one or two lines on what you produced, with the link.

Update the workpad one last time, then end the run with `pidash run finish` as "Lifecycle" describes.
```
