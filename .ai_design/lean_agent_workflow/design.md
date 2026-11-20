# Lean agent workflow: configurable stages and lighter run prompts

**Status:** implemented in PR #3403 (2026-11-19), issues A, B and C; see §14 for deviations and the remaining ops steps. Reviewed against the code at `8e8062beb`.

**Scope:** `rust-api/` (server), `runner/` (the `pidash` CLI), `apps/web_new` and `packages/api-client` (project settings). The Django server (`apps/api/`) is out of scope and gets no changes. No database migration is needed.

**Depends on:** NEWPIDASH-305 ("AI Completed" terminal state, `completed` group). This design assumes it has shipped and does not specify it.

**Followed by:** `.ai_design/optimize_pidash_cli_prompt/` (the CLI section of the prompt). That work now lands after this one. See §11.

**How to read this as an implementer:** §4 lists the decisions. §5–§9 specify the change. §10 lists the tests. §13 splits the work into three issues, each with its files, steps and acceptance criteria. Appendix A has the section text to start from. Paths are relative to `rust-api/crates/` unless they start with `runner/`, `apps/` or `packages/`.

## 1. Problem

Test issue PIDASHCLOU-2 asked the agent to post one comment. It cost about 1.66M tokens. Sending the same request straight to `claude -p` cost about 25k.

The cost came from three places:

| Cause                                                          | Evidence                                                             |
| -------------------------------------------------------------- | -------------------------------------------------------------------- |
| Three fresh agent sessions for every issue                     | implement 451k + review 656k + test 551k tokens                      |
| A large prompt, re-read on every step                          | 77–122 KB of template per run; roughly 18–27 tool calls per run      |
| Review and test rediscover what the implement run already knew | each re-classifies the work and re-assembles the acceptance criteria |

The CLI section of the prompt (38 KB) is handled by the separate CLI design. This design handles the rest: how many runs an issue gets, and what each run is told.

## 2. Current state

### 2.1 The three stages are forced

- `services/src/prompting/sections/ending-run.md` tells the implement run to move every finished issue to In Review, "whether it produced a durable deliverable or only an answer".
- `services/src/orchestration/entries.rs` (`is_delegation_trigger`, `handle_issue_state_transition`) creates a run on any move into a ticking state.
- `services/src/prompting/sections/review-cycle.md` Step 3 sends an approved review to In Test, which creates the test run.

A ticking state is defined statically in `types/src/orchestration/phases.rs` (`PHASES`): the `started`, `review` and `test` groups, matched by group and by the exact names "In Progress", "In Review" and "In Test". One function, `is_ticking_state`, answers two different questions:

1. Does this state have an agent prompt? (Which recipe does a run here use?)
2. Do runs start here automatically? (Entry runs and timer ticks.)

Nothing in the project configuration can change either answer.

### 2.2 How a run ends today

The agent makes up to four separate calls: `pidash workpad update`, `pidash comment add`, `pidash issue patch --state`, and `pidash run yield`.

`run yield` (`api/src/v1_work_items/handlers_actions.rs`, `yield_inner`) only writes keys into `agent_run.done_payload`: `status`, `yielded_at`, `note`, and `stop_ticking`. The clock reads that payload when the run ends (`services/src/orchestration/clock.rs`, `on_run_ended`). No outcome stops the clock; only `stop_ticking: true` does.

### 2.3 The per-run prompt

Raw template sizes for the software work type:

| Recipe        | Total  | CLI section | Everything else |
| ------------- | ------ | ----------- | --------------- |
| `coding-task` | 122 KB | 38 KB       | 84 KB           |
| `review`      | 77 KB  | 38 KB       | 39 KB           |
| `test`        | 83 KB  | 38 KB       | 45 KB           |

In the review and test recipes, the procedure for the stage itself is small (about 8 KB for review). The rest is shared machinery: `ending-run` (10.6 KB), `task-lifecycle` (6.6 KB), `session-framing` (4.8 KB), `blocking` (3.4 KB).

Specific waste:

- **Next-state rules in five places.** `task-lifecycle`, `ending-run`, `review-cycle` Step 3, `test-cycle` Step 3, and the end-of-run recipes in `pidash-cli`.
- **Rediscovery.** The implement run classifies the deliverable. Review then "decides what kind of review this is" from the payload, the description and the working directory. Test then "identifies the user and chooses the kind". Test Step 0 assembles acceptance criteria from five sources and fetches the workpad with `pidash workpad get` although the prompt already inlines it.
- **Overlapping inputs.** Review and test prompts inline the comment thread, the prior run's payload as pretty-printed JSON, and the full workpad.
- **Duplicated blocks.** The 2 KB branch-sync procedure appears nearly word for word in `work_types/software/review.md` and `work_types/software/test.md`.
- **Many endings.** Review has six outcome branches and test has seven, each with its own combination of state move, yield outcome and `--stop-ticking`.
- **Workpad write-back.** Review and test must rewrite the whole workpad, carrying the implementation sections forward unchanged, to update one block.
- **Rare procedures always present.** 6.5 KB of `analyze-and-scope` is the split-into-child-issues procedure; 2.8 KB is comment-writing examples.

## 3. Goals and non-goals

**Goals**

1. A project decides which stages run an agent. The default lets the agent decide per issue.
2. A trivial issue costs one run.
3. Review and test each have one short procedure that still handles a complicated code review or test.
4. In Progress is the main run. It hands review and test what they need, so they do not rediscover it.
5. Every run ends with one command.
6. Prompt size is held down by a test.

**Non-goals**

- Changing the CLI section of the prompt beyond its end-of-run recipes (the CLI design does that).
- A separate, clean agent profile. Agents keep the user's own settings and tools.
- A per-issue override of the stage setting. It can be added later.
- Creating the AI Completed state (NEWPIDASH-305).
- A feature flag or a v1/v2 prompt. The new prompt replaces the old one outright.
- Changing the legacy template seeding in `services/src/prompting/seed.rs` and `prompting/fragments/`. Those feed the `reseed_*` ops commands only and are not on the run path.

## 4. Decisions

| #   | Decision                                                                                                                                                                                    |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | The project setting has two modes: **Auto** and **Customize**. Customize has two toggles, In Review and In Test. In Progress is always on.                                                  |
| D2  | **Auto is the default**, including for existing projects.                                                                                                                                   |
| D3  | In Auto, both stages are available and the agent chooses the path per issue: forward, backward, or straight to AI Completed.                                                                |
| D4  | A disabled stage is a plain label. Entering it starts no run and no ticking. The prompt leaves it out.                                                                                      |
| D5  | The agent's path always ends at **AI Completed**, which stops ticking because it is outside the ticking states.                                                                             |
| D6  | A human can still start a run by hand in a disabled stage. It runs once, with that stage's own prompt, and starts no ticking.                                                               |
| D7  | Review and test each have **one** procedure. There is no light route and full route, and no branch on whether a PR exists. Effort scales with the hand-off.                                 |
| D8  | In Progress writes a structured **hand-off**. Review and test read it and do not read or write the workpad.                                                                                 |
| D9  | Review and test stay fresh sessions. They do not resume the implement session.                                                                                                              |
| D10 | One closing command, `pidash run finish`, replaces the four closing calls.                                                                                                                  |
| D11 | No rollout flag. `MIN_RUNNER_VERSION` is raised, but it is advisory today (old runners only show a banner), so the prompt also carries a three-line fallback for a runner without `finish`. |
| D12 | The setting is project-level only.                                                                                                                                                          |
| D13 | Server changes are Rust only. Rust prompt sections stop mirroring the Django ones.                                                                                                          |
| D14 | In Auto, the hand-off's suggested next stage is advice. A review run may still send work to In Test, or skip it.                                                                            |

## 5. The stage setting

### 5.1 Storage

One new key in `projects.settings`, the Rust-owned JSON bag defined in `services/src/domain/project_settings.rs`. That module's header lists the four steps for adding a key; follow them. No migration and no entity change.

```json
{ "agent_stages": { "mode": "auto", "review": true, "test": true } }
```

- `mode`: `"auto"` or `"custom"`. Required when the key is written.
- `review`, `test`: booleans, default `true`. Read only when `mode` is `"custom"`. They are stored even in Auto so the toggles reappear as the user left them.
- A missing key reads as `{"mode": "auto", "review": true, "test": true}`. This makes Auto the default for every existing project with no backfill.
- Role: `SettingsRole::ProjectAdmin`, like `issue_run_policy`.
- Validation in `check_key_value`: the value must be an object; `mode` must be one of the two strings; `review` and `test`, when present, must be booleans; any other key is `InvalidValue`.

Types to add in `project_settings.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStagesMode { #[default] Auto, Custom }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentStages { pub mode: AgentStagesMode, pub review: bool, pub test: bool }
// Default: { mode: Auto, review: true, test: true }

impl AgentStages {
    /// Whether the review stage runs an agent automatically.
    pub fn review_enabled(&self) -> bool { self.mode == AgentStagesMode::Auto || self.review }
    pub fn test_enabled(&self) -> bool { self.mode == AgentStagesMode::Auto || self.test }
    pub fn agent_decides(&self) -> bool { self.mode == AgentStagesMode::Auto }
}
```

The module's guidance says to use a column when a setting filters across many projects, naming the tick sweep. That does not apply: the sweep selects armed tickers, and a ticker is disarmed when its issue enters a disabled stage (§6.2). The setting is only read per issue.

### 5.2 Carrying the setting to the clock

Every path that decides about ticking already loads `ProjectClockPolicy` (`services/src/orchestration/clock.rs`). Add two fields to it:

```rust
pub struct ProjectClockPolicy {
    // ... existing five fields ...
    /// `settings.agent_stages` resolved: does In Review run automatically?
    pub review_enabled: bool,   // Default::default() must yield true
    pub test_enabled: bool,     // Default::default() must yield true
}
```

`ProjectClockPolicy` derives `Default` today; replace the derive with a manual `impl Default` so both new fields default to `true`.

Each place that builds the policy from project columns also selects `settings` and fills the two fields through one helper, `ProjectClockPolicy::with_stages(self, settings: &serde_json::Value) -> Self`, which calls `parse_row_settings` and falls back to enabled on a parse error. The construction sites:

- `services/src/orchestration/dispatch.rs` (`PROJECT_CLOCK_POLICY_SQL`)
- `services/src/orchestration/entries.rs` (`CLOCK_POLICY_SQL`, `clock_policy`)
- `services/src/runner_runs/failure_effects.rs`
- `jobs/src/issue_transition.rs` (`ClockPolicyRow`)
- `jobs/src/orchestration/fire_tick_seam.rs`, `runner_seams.rs`, `scheduler_dispatch.rs`
- `api/src/app_issues/handlers_reads.rs`, `handlers_core.rs`, `handlers_archive.rs`
- `api/src/app_intake/issues.rs`
- `api/src/v1_work_items/handlers_actions.rs`

Find them all with `grep -rn "ProjectClockPolicy {" --include=*.rs .`; every non-test hit must set the new fields from the project row.

### 5.3 API and UI

- **API:** nothing new. The key is read and written through the existing project endpoints, which already render and merge `settings` (`api/src/app_project/handlers_project.rs`).
- **Contract:** add `agent_stages` to the `ProjectSettings` schema in `packages/api-client/src/contracts/projects.ts`.
- **UI:** `apps/web_new/src/features/project-settings/general/components/ExecutionSection.tsx` gains a "Stages" block under "Automated runs", with its form values and save mapping in the same feature's `rules.ts`.
  - A select with two options: "Auto — the agent decides which stages a work item needs" and "Customize".
  - Under Customize, a dropdown with three checkable items. In Progress is checked and disabled. In Review and In Test toggle.
  - Helper text: "A stage that is off is only a label. Work items can still be moved there, but no agent run starts unless you start one."
  - The block stays usable when "Automated runs" is off: a person moving a work item into a stage still starts one run then, so the switches still matter. It is disabled only for someone who cannot edit the project.

## 6. Server behaviour

### 6.1 Two questions, two functions

`is_ticking_state` is split by the question being asked. Add to `types/src/orchestration/phases.rs`:

```rust
/// Which optional stages run automatically in this project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageSwitches { pub review: bool, pub test: bool }

/// True when entry runs and ticks happen in `state`: it is a registered
/// phase, and that phase is In Progress or is switched on.
pub fn stage_runs_automatically(state: Option<&StateRef<'_>>, switches: StageSwitches) -> bool
```

`types` cannot depend on `services`, so the function takes the plain `StageSwitches`; `ProjectClockPolicy` gets a `stage_switches()` accessor.

| Function                                            | Question                                             | Reads                               |
| --------------------------------------------------- | ---------------------------------------------------- | ----------------------------------- |
| `phase_config_for`, `template_name_for` (unchanged) | Does this state have an agent prompt, and which one? | static `PHASES`                     |
| `stage_runs_automatically` (new)                    | Do entry runs and ticks happen here?                 | `PHASES` and the project's switches |

`is_ticking_state` is removed once every caller has moved to one of the two, so no caller is left on the ambiguous name.

Call sites and what each becomes:

| Call site                                                                                                                                                | Becomes                                                                                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `services/src/orchestration/entries.rs`: `is_delegation_trigger`; `from_ticking` / `to_ticking` in `handle_issue_state_transition`                       | `stage_runs_automatically`                                                                                                                                                                                     |
| `services/src/orchestration/entries.rs`: the clock block in `handle_issue_comment`                                                                       | `stage_runs_automatically`                                                                                                                                                                                     |
| `services/src/orchestration/clock.rs`: `run_ended_guard`, the re-tick check, and any other `is_ticking_state(issue.state…)`                              | `stage_runs_automatically(issue.state, issue.policy.stage_switches())` — `ClockIssue` already carries the policy                                                                                               |
| `services/src/orchestration/dispatch.rs`: `wait_rearms`, the auto-pause probe, the two bounce checks                                                     | `stage_runs_automatically`; `wait_rearms` gains a switches parameter                                                                                                                                           |
| `jobs/src/tasks_ticker/fire_tick.rs`: the `is_ticking_state` trait method and its guard; `jobs/src/orchestration/fire_tick_seam.rs`, `fire_tick_live.rs` | the trait method takes the policy; implementations call `stage_runs_automatically`                                                                                                                             |
| `api/src/runner_runs/dispatch.rs`: `let ticking = …` in Run AI                                                                                           | `stage_runs_automatically`                                                                                                                                                                                     |
| `services/src/app_issues/serializers_detail.rs`: `can_re_tick`                                                                                           | `stage_runs_automatically`                                                                                                                                                                                     |
| `services/src/domain/issue_assignment.rs`: the bounce-arm check                                                                                          | `stage_runs_automatically`, loading the project's switches                                                                                                                                                     |
| `services/src/assistant/tools_runs.rs` and `api/src/turn/tools_local.rs`: the assistant's "start a run by moving to a state" tool                        | `stage_runs_automatically`. A move into a disabled stage starts no run, so the tool must answer with its existing non-ticking error. The duplicate `PHASES` table in `tools_runs.rs` stays as the static part. |
| `services/src/orchestration/creation.rs`: `phase_kind_for_issue`, `select_parent_for_next_run`, the cadence lookup                                       | unchanged (`phase_config_for` / `template_name_for`)                                                                                                                                                           |
| `api/src/prompting/mod.rs`: preview template name                                                                                                        | unchanged                                                                                                                                                                                                      |
| `jobs/src/tasks_cleanup/workspace_seed.rs`                                                                                                               | `phase_config_for(..).is_some()`; it classifies seed data and has no project policy                                                                                                                            |

### 6.2 Entering a disabled stage

A move into a disabled stage takes the existing non-ticking branches of `handle_issue_state_transition`:

- no run is created;
- if the issue came from a stage that runs automatically, the clock gets the existing `left_bucket` event and goes dormant;
- the outcome reason is `not-a-trigger-state`.

No new branch is needed. The two booleans computed at the top of the function change source, and the rest follows.

### 6.3 Manual runs in a disabled stage

Both human entry points already handle "the issue is not ticking" by dispatching a bare run and leaving the clock alone:

- **Run AI** (`api/src/runner_runs/dispatch.rs`): "otherwise the clock is untouched and the dispatch runs bare".
- **Comment & Run** (`services/src/orchestration/entries.rs`, `handle_issue_comment`): the clock block is skipped and `create_continuation_run` still runs. The state must be in `CONTINUATION_ELIGIBLE_GROUPS`, which already includes the review and test groups.

With the call sites in §6.1 switched, a disabled stage takes that same path. The recipe comes from `phase_kind_for_issue`, which is unchanged, so the run uses the stage's own prompt (D6). When the run ends, `run_ended_guard` returns `not-in-bucket` and the clock stays dormant.

The prompt for such a run says so (§9.3), because staying and waiting has no meaning where nothing re-runs.

### 6.4 Changing the setting on a live project

Saving the setting never starts, stops or reschedules anything by itself. The ticker of an issue sitting in the stage stays armed through the change, and the pre-fire guard decides at each tick.

- **A stage is turned off while issues sit in it.** Nothing is swept. A run already in flight finishes normally. When the next tick for such an issue comes due, the pre-fire guard in `fire_tick.rs` finds the stage off and **skips** the tick: it creates no run, spends no budget (`tick_count` unchanged), leaves the ticker enabled with no disarm reason, and sets `next_fire_at` one normal interval ahead, exactly as a fired tick would. The issue stays where it is.
- **A stage is turned back on.** Nothing happens at save time. The next tick that comes due fires as usual, so the issue resumes on its existing schedule. Example: next tick due in 2 hours, stage turned off, turned on 1 hour later, the tick fires 1 hour after that. If the stage stayed off for 5 hours, the ticks at hours 2 and 4 were skipped and the next fires at hour 6. No run fires immediately, so one settings change cannot cause a burst of runs.
- **An issue moved into the stage while it was off** has no armed ticker (§6.2) and stays idle after the stage is turned on, until a person moves it or presses Run AI.
- **Leaving the stage** still disarms the ticker through `left_bucket`, whether the stage is on or off.

Consequences for the implementation:

- The guard does not disarm on a disabled stage, so no `left_ticking_state` stop is created by a switch-off, and the "stage switched back on" branch in `on_retick` and `can_re_tick` (`stage_stopped`) is removed. Re-tick returns to its earlier behaviour, including `budget_not_exhausted` for an issue in Paused.
- The issue detail hides the next-tick time while the issue's stage is off; it reads the same `stage_runs_automatically` answer.
- A skipped tick is a cheap read per parked issue per interval; that cost is accepted.

### 6.5 Guarding agent moves

The prompt does not name a disabled stage, but an agent could still move an issue into one. The issue would then sit there with no run and nothing to tell a human why.

The `finish` endpoint (§8.1) therefore refuses a target state for which `phase_config_for` is `Some` and `stage_runs_automatically` is false. It returns 400 with the list of valid targets. A human moving an issue is never refused, and `pidash issue patch --state` is left as it is.

## 7. Paths

The agent's allowed moves, by setting. "AI Completed" is the state from NEWPIDASH-305.

| Setting                | Forward path                                                                        | Backward moves                                 |
| ---------------------- | ----------------------------------------------------------------------------------- | ---------------------------------------------- |
| Customize, both off    | In Progress → AI Completed                                                          | none                                           |
| Customize, review only | In Progress → In Review → AI Completed                                              | In Review → In Progress                        |
| Customize, test only   | In Progress → In Test → AI Completed                                                | In Test → In Progress                          |
| Customize, both on     | In Progress → In Review → In Test → AI Completed                                    | In Review → In Progress; In Test → In Progress |
| Auto                   | any forward move, including In Progress → AI Completed and In Review → AI Completed | any backward move                              |

Rules that hold in every setting:

- The agent never moves an issue to Done or Cancelled. Done is a human's confirmation of AI Completed.
- Blocked is for "I need a human", not for defects.
- A defect found in review or test goes back to In Progress with the open items listed.
- Ticking keeps its purpose: each tick re-checks the goal in whatever enabled stage the issue is in, and the run budget bounds the total.
- A project with no "AI Completed" state (a user deleted or renamed it) falls back as NEWPIDASH-305 specifies: the run stays in its current stage and finishes with `waiting_on_human`, which stops the clock.

### 7.1 What Auto tells the agent

In Auto, the In Progress prompt states the choice and what it costs. The text is in Appendix A.1. The rule it gives:

- **In Review** when the work product can be wrong in a way a careful read would catch: a code change, a document someone will act on, a change to data or configuration.
- **In Test** when the work product has behaviour that can be exercised from its user's side.
- **AI Completed** when neither applies: answering a question, posting a comment, reporting a status or a finding.
- When unsure between review and none, choose review.

Review and test runs in Auto have the same freedom (D14): a review may send the work to In Test, or straight to AI Completed when there is nothing to exercise.

## 8. The closing command and the hand-off

### 8.1 `pidash run finish`

One command ends an issue run. It replaces the separate `workpad update` (for review and test), `comment add`, `issue patch --state` and `run yield` calls.

```sh
pidash run finish --to "<state name>" --result <result> \
  [--comment-file <path>] [--handoff-file <path>] [--note "<one line>"] [--run-id <id>]
```

| Argument         | Meaning                                                                                                      |
| ---------------- | ------------------------------------------------------------------------------------------------------------ |
| `--to`           | Target state by exact name. Pass the current state's name to stay.                                           |
| `--result`       | `done`, `progressed`, `waiting_on_external`, `waiting_on_human`, or `blocked`.                               |
| `--comment-file` | Markdown posted as an issue comment: the answer, the findings, the test results, or the question to a human. |
| `--handoff-file` | In Progress runs only. The hand-off (§8.2).                                                                  |
| `--note`         | One line for the run history. Truncated to 2000 characters like `yield`.                                     |
| `--run-id`       | Defaults to `PIDASH_RUN_ID`, as `yield` does.                                                                |

**CLI:** add `Finish(FinishArgs)` to `RunCmdCommand` in `runner/src/cli/run_cmd.rs`, next to `Yield`. It reads the two files, and sends one request with the `X-Pi-Dash-Run-Id` header the client already attaches. On success it prints one line: the new state and whether the clock is running.

**Endpoint:** `POST workspaces/{slug}/agent-runs/{run_id}/finish/`, registered next to the yield route in `api/src/v1_work_items/routes.rs`, implemented in `handlers_actions.rs` next to `yield_inner`.

Request body:

```json
{ "to": "In Review", "result": "done", "comment": "…markdown…", "handoff": "…markdown…", "note": "…" }
```

Authorization is the same as `yield_inner`: authenticated caller, workspace member, `run_belongs_to`, run still active, header run id matches the path. A scheduler run (no issue) is refused with 400; scheduler runs keep using `yield`.

Steps, in this order:

1. **Validate.** Resolve `to` among the project's states by name (case-sensitive, non-triage). Unknown name: 400 with the available names. Disabled stage: 400 per §6.5. `result` must be one of the five. `handoff` on a run whose `phase_kind` is not `coding-task`: 400. A `coding-task` run moving to In Review or In Test with no `handoff`, and no earlier hand-off stored on this issue: 400 with a message naming `--handoff-file`.
2. **Write the payload.** Merge into `agent_run.done_payload`: `status` (the result), `yielded_at`, `note`, `stop_ticking` (derived, below), and `stage_handoff` when given. This is the same write `yield_inner` does, with two more keys. The key is `stage_handoff` because "handoff" already names the project-move handoff in `orchestration/creation.rs`.
3. **Post the comment**, when given, through the same operation the v1 comment-create handler uses, attributed to the run as agent comments are today. Then record `finish_comment_id` in the payload.
4. **Post the hand-off as a comment**, when given and when the target is In Review or In Test, so humans see what review and test will read. Record `handoff_comment_id` in the payload.
5. **Move the issue**, when `to` differs from the current state, through `operations::issues::update` with `moved_by_run` set to this run, exactly as `handlers_core.rs` does for an agent's state patch. This fires the transition and, for an enabled stage, creates the entry run.

The steps are not one database transaction, because the comment and issue operations own their transactions. They are ordered so a failure leaves a state the agent can retry from, and a retry is safe: step 3 is skipped when `finish_comment_id` is already set, step 4 when `handoff_comment_id` is set, and step 5 is a no-op when the issue is already in the target state.

Response: `200` with `{ "run": "<id>", "state": "<name>", "result": "<result>", "ticking": <bool> }`.

**Stopping the clock is derived, not requested.** The prompt no longer mentions `--stop-ticking`.

| Result                                                 | `stop_ticking` written | Effect                                                                                        |
| ------------------------------------------------------ | ---------------------- | --------------------------------------------------------------------------------------------- |
| `done`, moved to another enabled stage                 | false                  | the new stage's entry run                                                                     |
| `done`, moved to AI Completed or any non-ticking state | false                  | stops, because the issue left the ticking states                                              |
| `done`, not moved                                      | false                  | keeps ticking (a stage that is satisfied but has nowhere to go should use `waiting_on_human`) |
| `progressed`, `waiting_on_external`                    | false                  | keeps ticking                                                                                 |
| `waiting_on_human`, `blocked`                          | true                   | stops; a human reply, Run AI, a state move or Re-tick re-arms it                              |

No change to `on_run_ended` is needed: it already honours `done_payload.stop_ticking`.

`pidash run yield` and its endpoint are unchanged. Scheduler prompts keep using it.

**Old runners (D11).** `MIN_RUNNER_VERSION` is raised to the release that contains `finish`, in the same deploy as the new prompt. That setting only shows a banner today, so the lifecycle section also carries a fallback (Appendix A.1): if `pidash run finish` is not a known command, post the comment with `pidash comment add`, move with `pidash issue patch --state`, report with `pidash run yield`, and say in the comment that the runner needs updating. In that case no hand-off is stored, and review and test work from the comment thread.

### 8.2 The hand-off

In Progress ends by writing a short hand-off to a file and passing it with `--handoff-file`.

```markdown
### Hand-off

**Produced:** <what, and where it lives: PR URL, document path, comment, record changed>
**Acceptance criteria:**

- <criterion>
  **How to verify:** <the commands to run or the flow to exercise, from the user's side>
  **Review focus:** <the parts most likely to be wrong, and why>
  **Next stage:** <state> — <one-line reason>
```

The server treats it as opaque markdown, capped at 20,000 characters.

**Reading it.** The review and test prompts get a new context value, `handoff`: the `stage_handoff` of the most recent `coding-task` run on the issue that has one. This is one new bundle query in `services/src/orchestration/creation.rs`, next to the other `BUNDLE_*` constants:

```sql
SELECT done_payload->>'stage_handoff' FROM agent_run
WHERE work_item_id = $1 AND phase_kind = 'coding-task'
  AND done_payload ? 'stage_handoff'
ORDER BY created_at DESC LIMIT 1
```

A direct query is used instead of the existing parent-payload resolution (`resolve_parent_payload`), because that follows the run's parent or the ticker's stashed resume parent and does not always land on the latest In Progress run after a hand-back. The hand-off is inlined as markdown. The pretty-printed JSON `parent_done_payload` is dropped from the review and test intros.

When the query returns nothing (a human moved the issue straight into the stage, or an old runner used the fallback), `handoff` is empty and the stage prompt says to work from the issue and what it links to.

The hand-off replaces three things in today's prompt:

- the "Acceptance Criteria / How to Test" comment from `implementation.md` Step 7 (step 4 above posts the hand-off as that comment);
- review Step 1, "decide what kind of review this is";
- test Steps 0 and 1, "gather the acceptance criteria" and "identify the user, and choose the kind".

### 8.3 The workpad belongs to In Progress

Review and test no longer read or write the workpad (D8).

- **Into review and test:** the prompt inlines the issue, the hand-off, the stage history and the comment thread. `workpad_body` is no longer rendered in those recipes.
- **Out of review and test:** findings and open items go in the comment passed to `finish`. The next In Progress run already receives the comment thread, and its prompt says to treat the latest review or test comment as its open items.
- **Stage history:** a new context value, `stage_history`, rendered by the server from the issue's last 10 runs, oldest first: stage, result, note, date. One more bundle query:

```sql
SELECT phase_kind, done_payload->>'status', done_payload->>'note', created_at
FROM agent_run WHERE work_item_id = $1 AND id != $2
ORDER BY created_at DESC LIMIT 10
```

It replaces the hand-maintained `History` list in the workpad's `Path to done` block, and it is rendered in all three recipes.

For In Progress, the workpad stays as its memory between ticks, in a shorter form: `Path to done` (goal, open items), `Plan`, and `Notes`. The `Phase`, `Progress Checkpoints`, `Autonomy / Escalation`, `Analysis`, `Acceptance Criteria`, `Validation` and `Confusions` blocks and the seven-item completeness checklist are removed. The analysis and acceptance criteria now live in the hand-off.

## 9. The prompts

### 9.1 One procedure for review, one for test

Each stage is one question and one loop. The text is in Appendix A.2 and A.3.

- **Review:** is the work product correct and complete against the goal, judged by reading it?
- **Test:** does it work for whoever uses it, judged by using it?

The loop is the same in both:

1. Read the hand-off.
2. Examine the work product.
3. Confirm each finding is real.
4. Fix what is trivial.
5. Finish with one of three verdicts.

Effort scales because the hand-off is specific, not because the prompt forks. A posted answer takes one read. A large PR gets a full review under the same text.

The rules kept are the ones that change behaviour on hard work:

- Judge the branch synced with its base, never a stale one.
- Never report a finding you have not confirmed.
- A test verdict needs first-hand observation. Green CI is supporting evidence, not a verdict.
- Defects go back to In Progress as open items.
- No "nothing changed" comments.

Three verdicts replace the six and seven outcome branches:

| Verdict        | Finish with                                                           |
| -------------- | --------------------------------------------------------------------- |
| Pass           | `--to <next enabled stage or AI Completed> --result done`             |
| Defects        | `--to "In Progress" --result done`, findings in the comment           |
| Cannot proceed | stay; `--result waiting_on_external`, `waiting_on_human` or `blocked` |

### 9.2 Recipes

`services/src/prompting/recipes.rs` becomes:

```rust
KIND_CODING_TASK: [ S("intro"), SLOT_CONTEXT, S("relationships"), S("run-framing"), S("pidash-cli"),
                    S("lifecycle"), S("default-posture"), S("autonomy"), S("analyze-and-scope"),
                    S("workpad"), SLOT_EXECUTE, S("implementation"), S("blocking") ]
KIND_REVIEW:      [ S("review-intro"), SLOT_CONTEXT, S("run-framing"), S("pidash-cli"),
                    S("lifecycle"), S("review"), SLOT_REVIEW, S("blocking") ]
KIND_TEST:        [ S("test-intro"), SLOT_CONTEXT, S("run-framing"), S("pidash-cli"),
                    S("lifecycle"), S("test"), SLOT_TEST, S("blocking") ]
KIND_SCHEDULER:   unchanged
```

The scheduler recipe keeps `session-framing` and `guardrails`, so those two section files stay. Their issue-run branches are removed, which leaves them scheduler-only.

### 9.3 Section changes

| Section file                                                                                                                                                  | Action                                                                                                                                                                                                                                                                                                                      | Target size         |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- |
| `lifecycle.md`                                                                                                                                                | **New**, `locked`. Replaces `task-lifecycle`, `state-routing`, `ending-run`, and Step 3 of `review-cycle` and `test-cycle`. Renders the path from `stages`, this stage's allowed moves, the Auto guidance, the budget, the `finish` command and the old-runner fallback. Has a branch for a manual run in a disabled stage. | 2.5–3 KB rendered   |
| `run-framing.md`                                                                                                                                              | **New**, `locked`. The issue-run half of `session-framing`: the trigger for this run and the working rules. Budget text moves to `lifecycle`.                                                                                                                                                                               | 1.5 KB              |
| `review.md`                                                                                                                                                   | **New**, `overridable`. Replaces `review-cycle`.                                                                                                                                                                                                                                                                            | 1.5 KB              |
| `test.md`                                                                                                                                                     | **New**, `overridable`. Replaces `test-cycle`.                                                                                                                                                                                                                                                                              | 1.8 KB              |
| `workpad.md`                                                                                                                                                  | **New**, `overridable`. Replaces `workpad-setup` and `workpad-template` for In Progress.                                                                                                                                                                                                                                    | 2 KB                |
| `blocking.md`                                                                                                                                                 | Rewritten; absorbs the issue-run rules from `guardrails`. Ends with `finish … --result blocked`.                                                                                                                                                                                                                            | 1.5 KB              |
| `review-intro.md`, `test-intro.md`                                                                                                                            | Rewritten: issue, `{{ handoff }}`, `{{ stage_history }}`, comments. `parent_done_payload` and the no-workpad paragraph removed.                                                                                                                                                                                             | 1 KB each plus data |
| `intro.md`                                                                                                                                                    | `{{ stage_history }}` added.                                                                                                                                                                                                                                                                                                | —                   |
| `analyze-and-scope.md`                                                                                                                                        | Trimmed from 18 KB to about 8 KB. The split procedure keeps the independence test and the essential steps. One comment example is kept, not three. The six-point analysis becomes "you will record this in the hand-off".                                                                                                   | 8 KB                |
| `implementation.md`                                                                                                                                           | Trimmed where it repeats lifecycle rules. Step 7 becomes "write the hand-off".                                                                                                                                                                                                                                              | 5 KB                |
| `pidash-cli.md`                                                                                                                                               | Only the end-of-run recipes change, to the single `finish` command, and the `run yield` reference is limited to scheduler runs. The rest waits for the CLI design.                                                                                                                                                          | unchanged           |
| `work_types/software/execute.md`, `review.md`, `test.md`                                                                                                      | Trimmed per §9.4.                                                                                                                                                                                                                                                                                                           | 8 KB, 1 KB, 2.5 KB  |
| `work_types/general/*.md`                                                                                                                                     | Edited only to match the new verdicts and the hand-off.                                                                                                                                                                                                                                                                     | unchanged           |
| `task-lifecycle.md`, `state-routing.md`, `ending-run.md`, `review-cycle.md`, `test-cycle.md`, `workpad-context.md`, `workpad-setup.md`, `workpad-template.md` | **Deleted**, along with their `include_str!` entries in `registry.rs`.                                                                                                                                                                                                                                                      | —                   |

New context values, added to `IssueContextInput` and `build_context` in `services/src/prompting/context.rs`, filled from `RenderBundle` in `orchestration/creation.rs`, and added to the sample contexts in `prompting/validation.rs` and the preview path in `api/src/prompting/mod.rs`:

| Key             | Shape                                                                                                                                                                                                                                                                                                                                                                   |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `stages`        | `{ "agent_decides": bool, "review": bool, "test": bool, "path": ["In Progress", "In Review", "In Test", "AI Completed"], "current_runs_automatically": bool, "final_state": "AI Completed" or null }`. `path` lists only enabled stages plus the final state, using the project's actual state names. `final_state` is null when the project has no AI Completed state. |
| `handoff`       | string, empty when none                                                                                                                                                                                                                                                                                                                                                 |
| `stage_history` | string, pre-rendered, one line per run; empty when none                                                                                                                                                                                                                                                                                                                 |

`workpad_body` stays in the context for `coding-task`. `parent_done_payload` stays in the context for workspace overrides that reference it, but no built-in section uses it.

### 9.4 Work-type slots

The stage text is the same for every work type. The work-type slot carries only what is specific to that type.

- **`software/review.md`:** where the code review lives, the sync rule, and that fixes may be pushed to the branch. The CODE / DESIGN / DESIGN_THEN_CODE list is removed; the hand-off says what was produced. See Appendix A.4.
- **`software/test.md`:** how to get somewhere the software runs, the rule that the repository's own checks are discovered and never assumed, one short paragraph on acting as the consumer (a program, a person at a screen, an operator, a reader), and the limits on fixing. The agent-close merge line is already removed by NEWPIDASH-305.
- **Branch sync:** the renderer has no include mechanism, so the sync procedure is stated once in full in `software/execute.md` under the heading "Branch sync", and compressed to a five-line rule in `software/review.md` and `software/test.md` (fetch, merge the base, never rebase or force-push, run the repo's checks, send back to In Progress when the conflict is too large to resolve with confidence).

### 9.5 Workspace overrides

Workspaces can override `overridable` sections (`prompt_section_override`, keyed by section key). This change affects them in two ways:

- **Deleted keys** (`review-cycle`, `test-cycle`, `workpad-setup`): their override rows no longer apply. The new sections use new keys on purpose, so an old override written against the old procedure is not silently applied to the new one.
- **Rewritten keys** (`analyze-and-scope`, `implementation`, `software.execute`, `software.review`, `software.test`, `general.*`): an existing override still wins and may name `pidash run yield` or workpad blocks that no longer exist. It keeps working through the fallback commands, but it defeats the savings.

After the deploy, run the existing ops command `revalidate_section_overrides`. It already flags rows with an unknown key as `needs_attention`. Extend `decide_revalidate` in `prompting/seed.rs` with a list of keys rewritten by this change, so active overrides on those keys are flagged too, with the detail "section rewritten by the lean workflow change; review this override". The release notes say the same.

### 9.6 Size budgets

Budgets are for the rendered software recipes, with the `pidash-cli` section excluded.

| Recipe        | Today (excluding CLI) | Budget |
| ------------- | --------------------- | ------ |
| `coding-task` | 84 KB                 | 50 KB  |
| `review`      | 39 KB                 | 12 KB  |
| `test`        | 45 KB                 | 13 KB  |

A unit test in `services/src/prompting/composer.rs` composes each recipe for the `software` work type with the first sample context from `validation::sample_contexts`, subtracts the rendered `pidash-cli` section, and fails when the remainder exceeds its budget. The constants live in one place with a comment: lower a budget when a section shrinks; do not raise one without a design note.

If a drafted section cannot meet its target without losing a rule from §9.1, the implementer raises the budget in the same PR and says why in the PR description. The budgets above are the plan, not a reason to cut a behaviour-changing rule.

## 10. Tests and measurement

### 10.1 Tests

Run with `cargo test -p <crate>` from `rust-api/`, `cargo test` from `runner/`, and `pnpm --filter web_new test` for the UI.

- **Settings** (`project_settings.rs`): default when the key is missing; each valid shape; each invalid shape returns `InvalidValue`; role check; `review_enabled` / `test_enabled` under Auto and Custom.
- **Policy** (`phases.rs`): `stage_runs_automatically` for In Progress, In Review, In Test, a same-group state with another name, and a non-phase state, under all four switch combinations.
- **Transitions** (`entries.rs`): entering a disabled stage from In Progress creates no run and applies `left_bucket`; entering an enabled stage behaves as today.
- **Manual runs**: Run AI and Comment & Run in a disabled stage create one run whose `phase_kind` is the stage's, and the clock is dormant after it ends.
- **Live change** (`fire_tick.rs`): a tick on an issue sitting in a stage that was turned off is skipped: no run, `tick_count` unchanged, ticker still enabled, `next_fire_at` moved one interval ahead. After the stage is turned on, the next due tick fires. Re-tick on an issue in Paused with budget left answers `budget_not_exhausted`.
- **Finish endpoint** (`handlers_actions.rs`): each result's `stop_ticking`; unknown state; disabled-stage target; hand-off on a non-coding run; missing hand-off on a move to review; retry after a simulated failure posts no duplicate comment; scheduler run refused.
- **Finish CLI** (`runner/src/cli/run_cmd.rs`): argument parsing, file reading, request body, missing run id error.
- **Prompt** (`composer.rs`): for each of the five settings in §7, the composed review, test and coding prompts name exactly the enabled stages; a disabled stage's name does not appear in the lifecycle section; the manual-run branch renders when `current_runs_automatically` is false; size budgets hold.
- **UI** (`ExecutionSection.test.tsx`, `rules.test.ts`): reading each stored shape onto the form, including a missing key; the save payload for each choice; In Progress cannot be toggled.

### 10.2 Fixtures

The goldens under `rust-api/fixtures/` were captured from the Django implementation, and several tests assert the Rust code matches them. This change makes Rust diverge on purpose (D13). For each fixture below, regenerate the affected entries from the Rust implementation and note in the fixture's `TRACE`/header that it no longer mirrors Django:

- `fixtures/prompting/FIX-recipes.json`, `FIX-registry.json`, `FIX-compose.json`, `FIX-context.json`, `FIX-turns.json`: recipes, the section list and tiers, composed output, context keys.
- `fixtures/orchestration/fx05_clock`, `fx07_entries`, `fx08_dispatch`: only cases that construct a policy need the two new fields; expected outcomes do not change with both stages enabled.
- `fixtures/v1_work_items/handlers/F18-11.work_items.json`: add the `finish` calls.

`registry.rs` documents a test asserting every section mirror is byte-identical to its Python source. If that test exists in CI at implementation time, remove it for the files this change touches and update the module comment; the Rust sections are the source from here on.

### 10.3 Measurement

On the test instance (`pidash-test.service`; never the production daemon), re-create PIDASHCLOU-2 (one comment requested) and one real code-change issue. For each, record runs, steps per run and tokens from `~/.local/share/pidash-test/runners/*/history/runs/`.

| Case                                             | Today                      | Target                                                                  |
| ------------------------------------------------ | -------------------------- | ----------------------------------------------------------------------- |
| One-comment issue, Auto                          | 3 runs, about 1.66M tokens | 1 run                                                                   |
| One-comment issue, Customize with both stages on | 3 runs                     | 3 runs; review and test each under a third of today's tokens            |
| Code-change issue, Auto                          | 3 runs                     | 3 runs with fewer steps in each; the review still finds a seeded defect |

Token targets for a single run are not set here, because the 38 KB CLI section still dominates each prompt until the CLI design lands.

## 11. Relationship to the CLI prompt design

The CLI design (`.ai_design/optimize_pidash_cli_prompt/`, issues NEWPIDASH-320, 321, 325) now lands after this work. Four things in it need adjusting then. They are recorded here and not changed now:

1. **Its `PROMPT_PIDASH_CLI_V2` flag is dropped.** The decision for both designs is to replace the prompt outright.
2. **Its end-of-run recipes are superseded** by `pidash run finish`.
3. **Its workpad file (`.pidash/workpad.md`) applies to In Progress only**, since review and test no longer use the workpad.
4. **Rare procedures move to `pidash docs`.** This design condenses the split procedure and comment examples in place, because `pidash docs` does not exist yet. The CLI design can move them out.

## 12. Risks

| Risk                                                           | Mitigation                                                                                                                                                                                                                                                                  |
| -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| In Auto, the agent skips review on work that needed it.        | The guidance leans toward review when unsure. The choice and reason are in the hand-off and the run note, visible to humans. A human can move the issue to In Review, which starts a review run in Auto. A project that wants certainty uses Customize with both stages on. |
| Review and test lose context by not reading the workpad.       | The hand-off carries the acceptance criteria, how to verify, and the review focus. The measurement in §10.3 checks a seeded defect is still found before the work is called done.                                                                                           |
| A weak hand-off produces a weak review.                        | The In Progress prompt treats the hand-off as part of the deliverable. `finish` refuses a move to In Review or In Test from an In Progress run with no hand-off.                                                                                                            |
| `finish` fails partway.                                        | Steps are ordered and idempotent on retry (§8.1).                                                                                                                                                                                                                           |
| An old runner receives a prompt naming `finish`.               | The fallback in the lifecycle section; the `MIN_RUNNER_VERSION` banner.                                                                                                                                                                                                     |
| Workspace overrides carry the old procedure.                   | New keys for replaced sections; rewritten keys flagged by `revalidate_section_overrides` (§9.5).                                                                                                                                                                            |
| NEWPIDASH-305 slips.                                           | Issue C does not start before it. Issues A and B do not depend on it.                                                                                                                                                                                                       |
| The Rust prompt diverges from Django while both servers exist. | Intended (D13). Fixtures are re-based on Rust (§10.2).                                                                                                                                                                                                                      |

## 13. Implementation issues

Three issues. A and B are independent of each other. C needs both and NEWPIDASH-305.

### Issue A — Stage setting and server behaviour

**Areas:** rust-api, `packages/api-client`, `apps/web_new`.

**Steps**

1. `agent_stages` key, types, validation, role (§5.1).
2. `StageSwitches` and `stage_runs_automatically` in `types/src/orchestration/phases.rs` (§6.1).
3. Two fields on `ProjectClockPolicy`, manual `Default`, `with_stages`, `stage_switches`; fill them at every construction site (§5.2).
4. Move every `is_ticking_state` caller per the table in §6.1; remove `is_ticking_state`.
5. Pre-fire guard skips and reschedules the tick on a disabled stage, without disarming (§6.4).
6. Contract schema and the Stages control (§5.3).
7. Tests: settings, policy, transitions, manual runs, live change, UI (§10.1); fixture fields (§10.2).

**Acceptance criteria**

- A project with no `agent_stages` key behaves exactly as before this change.
- With Customize and In Review off: moving an issue to In Review creates no run and leaves the ticker dormant; Run AI there creates one `review` run; no tick follows it.
- With Customize and In Test off, the same holds for In Test.
- Turning a stage off with an issue sitting in it skips its ticks without disarming the ticker or spending budget; turning it back on starts nothing at save time, and the next due tick fires on the existing schedule.
- `grep -rn "is_ticking_state" rust-api/crates` returns nothing.
- The project settings page shows and saves the control; In Progress cannot be turned off.
- `cargo test` passes for `types`, `services`, `api` and `jobs`; `pnpm --filter web_new test` and `pnpm check:types` pass.

This is safe to ship alone. The default enables both stages, so behaviour changes only for a project that chooses Customize.

### Issue B — `pidash run finish` and the hand-off

**Areas:** `runner/`, rust-api.

**Steps**

1. `finish` endpoint and route (§8.1), including validation, derived `stop_ticking`, idempotent retry, and the §6.5 guard. If issue A has not merged, the guard is written against `stage_runs_automatically` and lands with whichever merges second.
2. `Finish` subcommand in `runner/src/cli/run_cmd.rs`.
3. `handoff` and `stage_history` bundle queries, `RenderBundle` fields, and context keys (§8.2, §8.3, §9.3). The built-in sections do not use them until issue C.
4. Tests: endpoint, CLI, context (§10.1); `F18-11` fixture.
5. Cut a runner release.

**Acceptance criteria**

- `pidash run finish --to "In Review" --result done --comment-file c.md --handoff-file h.md` from an In Progress run posts the comment, posts the hand-off, stores `stage_handoff`, moves the issue, and a review run is created.
- `--result waiting_on_human` leaves `stop_ticking: true` in the payload and the clock stops when the run ends.
- Re-running the same command after success changes nothing and posts nothing.
- A target that is a disabled stage returns 400 and lists the valid states.
- `pidash run yield` behaves as before.
- `cargo test` passes in `runner/` and for the `api` and `services` crates; `cargo clippy -- -D warnings` passes in `runner/`.

### Issue C — Prompt rewrite, budgets, measurement

**Areas:** rust-api. **Blocked by:** A, B, NEWPIDASH-305.

**Steps**

1. Write the new sections and rewrite or trim the others (§9.3, §9.4), starting from Appendix A.
2. Update `recipes.rs` and `registry.rs`; delete the removed section files.
3. Add `stages` to the context from the project's policy and states.
4. Extend `revalidate_section_overrides` (§9.5).
5. Size budget test (§9.6); prompt tests (§10.1); regenerate fixtures (§10.2).
6. Raise `MIN_RUNNER_VERSION` to the release from issue B in the deploy configuration.
7. Run the measurement in §10.3 on the test instance and post the table on the issue.

**Acceptance criteria**

- No built-in issue-run section mentions `pidash run yield`, `--stop-ticking`, `pidash workpad` in a review or test recipe, or `parent_done_payload`.
- The composed prompts name only enabled stages, for each of the five settings in §7.
- The size budget test passes with the budgets in §9.6, or with adjusted budgets explained in the PR.
- On the test instance in Auto, a one-comment issue finishes in one run and ends in AI Completed.
- On the test instance, a code-change issue with a seeded defect is sent back to In Progress by the review run.
- `cargo test -p pidash-services` passes.

## Appendix A. Section text

Starting text for issue C. Template syntax is the renderer's (`{{ }}`, `{% if %}`, `{% for %}`).

### A.1 `lifecycle.md`

```markdown
---
key: lifecycle
title: Lifecycle
customizable: locked
---

## Where this task is, and how the run ends

This issue is one task with a goal. Each run is a fresh session that checks the goal from where the issue stands, then moves it on, sends it back, or waits. Nothing carries over between runs except the issue itself: its comments, its state{% if run.kind == "coding-task" %}, the workpad{% endif %} and the delivered work.

Path in this project: {{ stages.path | join(" → ") }}. You are in **{{ issue.state }}**.
{% if not stages.current_runs_automatically %}
Agent runs are switched off for this stage in this project. A person started this run by hand, and no run will follow it here. Do not wait: finish with a verdict and move the issue, or say exactly what a person must do.
{% endif %}
{% if stages.agent_decides %}
You choose the path. Each later stage is a separate, fresh agent run, so send the work on only when a second pass can add something:

- **In Review** — the work product can be wrong in a way a careful read would catch: a code change, a document someone will act on, a change to data or configuration.
- **In Test** — the work product has behaviour that can be exercised from its user's side.
- **{{ stages.final_state }}** — neither applies: a question answered, a comment posted, a status or finding reported.

When unsure between review and none, choose review. Any run may also send the task back to an earlier stage.
{% else %}
Move forward to the next state on the path when this stage's work passes. Move back to In Progress when it has defects.
{% endif %}

Never move an issue to Done or Cancelled; a person does that. Use Blocked only when you need a person, never for a defect.{% if not stages.final_state %} This project has no "AI Completed" state: when the task is finished, stay in the current state and finish with `waiting_on_human`.{% endif %}

End the run with one command, as your last action:

    pidash run finish --to "<state>" --result <result> --comment-file <path>{% if run.kind == "coding-task" %} [--handoff-file <path>]{% endif %}

- `done` — this stage's work is finished and you moved the issue.
- `progressed` — you did work and more remains in this stage. A later run continues.
- `waiting_on_external` — waiting on checks, a merge, or another issue. A later run re-checks.
- `waiting_on_human`, `blocked` — a person must act first. No run follows until they do.

Pass the current state's name to `--to` to stay. If `pidash run finish` is not a known command, the runner is out of date: use `pidash comment add`, `pidash issue patch {{ issue.identifier }} --state "<state>"` and `pidash run yield --outcome <result>` in its place, and say so in your comment.

{% if tick %}Budget: {{ tick.count }}{% if tick.cap is not none %} of {{ tick.cap }}{% endif %} runs used on this issue. The budget never changes which state is true.{% if tick.spent %} **This is the last run.** Leave the task ready to hand over and tell the person in your comment that no run will follow.{% endif %}{% endif %}
```

The renderer is minijinja, which supports the `join` filter and `not` used above.

### A.2 `review.md`

```markdown
---
key: review
title: Review
customizable: overridable
---

## Review

Your question: **is the work product correct and complete against the goal, judged by reading it?**

{% if handoff %}The hand-off above says what was produced, the acceptance criteria, and where to look first. Start there.{% else %}There is no hand-off: a person moved the issue here directly. Work from the issue and what it links to.{% endif %}

1. Read the work product itself, not the description of it.
2. Look for what is wrong or missing against the acceptance criteria. Give the "review focus" items the most attention.
3. Confirm each finding by re-reading the source. Drop any you cannot confirm.
4. Fix what is small and clearly right, where the work-type guidance allows it. Leave the rest as findings.
5. Write one comment: what you checked, confirmed findings, what you fixed, what still needs action. If nothing has changed since your last pass, write none.

Match the depth to the work. A short answer needs one careful read. A large change needs every changed file read in context.

Verdict:

- **Pass** — no unresolved findings. Move forward.
- **Defects** — findings you could not fix. Move back to In Progress; your comment is the open-items list.
- **Cannot proceed** — stay, and say what you are waiting on.
```

### A.3 `test.md`

```markdown
---
key: test
title: Test
customizable: overridable
---

## Test

Your question: **does this work for whoever uses it, judged by using it?**

You are the first user of this work, not its author or its reviewer. {% if handoff %}The hand-off above gives the acceptance criteria and how to verify.{% else %}There is no hand-off: derive the criteria from the issue and state your assumptions in your comment.{% endif %}

1. For each acceptance criterion, act on the work product the way its user would and observe the result.
2. Exercise the two or three neighbouring things the change could have broken.
3. Keep evidence for every result: output, a response, a quoted passage. A result you inferred is not a result.
4. Re-run anything that failed before reporting it. Drop what you cannot reproduce.
5. Write one comment: pass or fail per criterion with evidence, and any defects. If nothing has changed since your last pass, write none.

Other people's checks, including a green pipeline, support a verdict but are not one. If you cannot get somewhere to act as the user, say exactly what is missing and finish `blocked`; do not report a pass.

Verdict:

- **Pass** — every criterion observed to hold. Move forward.
- **Defects** — move back to In Progress; your comment is the open-items list.
- **Cannot proceed** — stay, and say what you are waiting on.
```

### A.4 `work_types/software/review.md`

```markdown
---
key: software.review
title: Reviewing software
customizable: overridable
---

### Software

The work product is usually a {{ repo.code_review_term }}; the hand-off links it. Read existing reviewer comments there{% if repo.provider == "github" %} (use the `gh` CLI){% endif %} and post your findings there. Fixes may be committed and pushed to its branch. A design document is reviewed where it lives; fix it by editing it.

Judge the branch synced with its base: `git fetch origin`, check out the branch, `git merge origin/<base>` (never rebase, never force-push), run the repository's own checks, and push the merge if it is not a no-op. If the base rewrote the code under review and you cannot resolve it with confidence, abort the merge and send the issue back to In Progress saying what changed. If the base has moved since an earlier verdict, review again.

Waiting on checks or on a merge is `waiting_on_external`.
```

### A.5 Hand-off instruction (in `implementation.md`, Step 7)

```markdown
### Step 7 — Write the hand-off

The hand-off is part of your deliverable. Later runs start cold and read only this, the issue and its comments.

Write it to a file and pass it with `--handoff-file`:

    ### Hand-off
    **Produced:** what you made and where it lives (link or path).
    **Acceptance criteria:** one line each.
    **How to verify:** the commands to run or the flow to exercise, from the user's side.
    **Review focus:** the parts most likely to be wrong, and why.
    **Next stage:** the state you are moving to, and a one-line reason.

A move to In Review or In Test is refused without it. For a task that ends at {{ stages.final_state }}, your answer goes in `--comment-file` and no hand-off is needed.
```

## 14. Implementation notes

Implemented in PR #3403 as one commit per issue (A, B, C).

**Deviations from the text above**

- `finish` lives in `api/src/v1_work_items/handlers_finish.rs`, not `handlers_actions.rs`. It posts comments and moves the issue by calling the existing v1 comment-create and issue-patch handlers with the run's `X-Pi-Dash-Run-Id`, so their gates, attribution and transition hooks apply unchanged.
- `finish` takes an optional `as_agent` (CLI `--as-agent`) for the comment's speaker label; it defaults to `AI Agent`.
- `finish` never refuses the issue's current state, even when that stage is switched off, so a manual run there can stay (§6.3 overrides the §6.5 refusal for that case).
- `finish` writes `status` and `stop_ticking` last, after the comments and the move succeed; only the hand-off is stored up front. A refused step therefore leaves no stop signal for a later `yield` to inherit.
- The retry guard stores a SHA-256 of each posted comment; a retry with different text posts the new text. Before posting, `finish` also looks for a live comment this run already posted with the same body, so a failed id write cannot cause a double post. Each payload write merges only its own keys (`done_payload || delta`).
- An issue run may not move its own issue into Done, Cancelled (any completed state other than "AI Completed") or a stage the project switched off. The rule is `agent_move_refusal` (types crate), applied by `finish` and by the v1 issue-patch gate for an issue run on its own issue, so the `issue patch --state` fallback is held to it too (this overrides §6.5's "left as it is"). Human moves and scheduler runs are not affected; staying in the current state is always allowed.
- An explicitly empty `comment` or `handoff` is refused (400), and the CLI refuses an empty `--comment-file` or `--handoff-file`. `waiting_on_human` and `blocked` need a comment (endpoint and CLI): they stop the clock until a person acts, so the run must say what that person has to do.
- `finish` moves the issue before posting anything: the hand-off is kept as `pending_stage_handoff`, the move runs, then the hand-off becomes `stage_handoff` and the comments are posted. A refused move leaves no comment and no hand-off that counts.
- The response's `ticking` reports what follows the run: a move into a stage that runs starts its entry run whatever the result (the run-ended check ignores a stop from a stage the issue left), unless a person switched the clock off or the budget is spent; staying needs a result that keeps the clock and a ticker that can fire.
- A tick on a stage that is switched off is skipped and rescheduled one interval ahead (`StageGate::Reschedule`, `skipped_tick_next_run_at`), without disarming or spending budget; the issue detail hides `next_run_at` and reports `enabled: false` while the stage is off. The earlier disarm and its Re-tick "stage switched back on" branch are removed (§6.4).
- The stage switches are read from the `agent_stages` key alone (`agent_stages_of`), so a bad value under another settings key never turns stages back on.
- Leaving any registered stage for one that does not run sends `left_bucket`, so a ticker armed before its stage was switched off is disarmed when the issue moves on. Leaving In Progress for a switched-off stage still records the resume parent. The stage switches are read only when a move or Comment & Run involves a stage.
- The old-runner fallback adds `--stop-ticking` to `yield` for `waiting_on_human` and `blocked`, and an In Progress run posts its hand-off as a comment. Review and test, with no stored hand-off, look for one in the comments.
- In Auto, the prompt offers In Review or In Test only when the project has that state.
- `revalidate_section_overrides` flags an override on a rewritten section only while its body still uses the retired procedure (`pidash run yield`, `--stop-ticking`, `parent_done_payload`, the removed workpad blocks). It also flags an `implementation` override that drops the hand-off step, which only that section carries. A rewritten override is not flagged again, and `--clear` clears an old flag.
- The hand-off query tests `done_payload->>'stage_handoff' IS NOT NULL` instead of the jsonb `?` operator.
- `stages.path` lists In Review or In Test only when the project has that state.
- The auto-pause guard in `dispatch.rs` keeps the static registry check (`phase_config_for`): only In Progress auto-pauses, and it always runs automatically.
- `phases.golden.json`'s `is_ticking_state` vector key is renamed `stage_runs_automatically`.
- The Django mirror contract tests for prompting are removed; the section-order difference is recorded as DIFF-54 in `.ai_design/validation/differences.md`.

**Measured prompt sizes** (software work type, excluding `pidash-cli`): coding-task 27.3 KB, review 7.9 KB, test 8.9 KB, against budgets of 50 / 12 / 13 KB.

**Not done in code (ops)**

- Cut a runner release containing `pidash run finish` (issue B step 5).
- Raise `MIN_RUNNER_VERSION` to that release in the deploy configuration (issue C step 6).
- Run `revalidate_section_overrides` after the deploy (§9.5).
- Run the measurement in §10.3 on the test instance (issue C step 7).
- NEWPIDASH-305 (#3388) has not merged. Until it does, every project renders with `final_state` null and finishes with `waiting_on_human`, per §7.
- The assistant's dispatch tool answers a switched-off stage with its own message, which never suggests the refused stage.
- The Rust prompt directories (`rust-api/crates/services/src/prompting/{sections,fragments,work_types}`) are in `.prettierignore`, like the Django ones: their Jinja whitespace is significant at render time.
