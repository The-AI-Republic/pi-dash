# Optimize how the `pidash` CLI reaches the agent

> Directory: `.ai_design/optimize_pidash_cli_prompt/`
>
> **Status:** implementation-ready. Not started. Revised 2026-11-20 after the
> lean agent workflow change merged (PR #3403); §0 lists what that changed
> here. Decisions are in §10. Work breakdown (three issues, server side in
> Rust only) is in §11, and a draft of the new core prompt section is in
> Appendix A.
>
> **Companion:** `review.md`, a line-by-line review of the current
> `pidash-cli` section with the evidence behind each change here (findings
> R1–R10). It was written before the lean agent workflow change and is kept
> as evidence; line numbers in it refer to the section as it was then.
>
> **Scope:** how an issue/scheduler agent run learns to use the `pidash` CLI
> and gets the issue data it works from. Covers:
>
> - the `pidash-cli` prompt section;
> - core commands re-explained in other sections;
> - the data the prompt inlines (issue, comments, states, workpad) and the CLI
>   instructions that make agents re-fetch it;
> - the environment contract between runner, CLI and prompt;
> - the `pidash` skill installed in agent home directories;
> - `pidash --help`.
>
> Goal: stop paying for the full CLI manual on every step, stop paying for
> fetches of data the agent already has, and make the prompt match what the
> runner and CLI actually do.
>
> **Server side is Rust only.** All server changes land in `rust-api/`
> (prompting, orchestration, envelopes, the workpad endpoint, config). The
> Django backend (`apps/api/`) is **out of scope**: its prompting code is not
> updated, and nothing here needs Django parity. References to `apps/api/`
> below describe today's behaviour (they were the evidence); they are not
> change targets.
>
> **Out of scope:**
>
> - Django (`apps/api/`), see above.
> - The runner's use of the user's own agent config (skills, MCP connectors,
>   settings). Agents need the user's resources for user tasks; we keep it.
> - Which stages an issue goes through, the end-of-run command
>   (`pidash run finish`) and the review/test hand-off. Those shipped with the
>   lean agent workflow design (`.ai_design/lean_agent_workflow/design.md`);
>   this design builds on them and does not change them.

---

## 0. What the lean agent workflow change means for this design

The lean agent workflow change (PR #3403, merged 2026-11-20) rewrote the
issue-run prompts around this design's target. It did not touch the
`pidash-cli` section beyond swapping the end-of-run command. The effects on
this design:

| Area                | Before                                                                     | Now                                                                                                                                                                                      |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Priority            | The CLI section was 36–50% of an issue-run prompt                          | It is about 55% of an In Progress prompt and about 80% of a review or test prompt (§2.1). It is now the largest single cost per step                                                     |
| Rollout             | New prompt behind `PROMPT_PIDASH_CLI_V2`, as a second section key          | No flag and no second key. `pidash-cli` is rewritten in place, as the lean change did for its sections (§7). The section is `locked`, so no workspace override can hold the old text     |
| Ending a run        | `comment add`, `issue patch --state`, `run yield`, owned by `ending-run`   | One command, `pidash run finish`, owned by the `lifecycle` section. `run yield` remains for scheduler runs and as the old-runner fallback (§6.6, §6.7)                                   |
| Workpad             | Used by every stage; inlined in review and test                            | In Progress only. Review and test read the hand-off, which is inlined. The workpad file and its envelope fields apply to `coding-task` runs only (§6.1); former decision D7 is withdrawn |
| Sections named here | `ending-run`, `task-lifecycle`, `workpad-setup`, `workpad-context`, cycles | Those are gone. The owners are now `lifecycle`, `workpad`, `run-framing`, `review`, `test` (§6.7)                                                                                        |
| Duplication (R7)    | Core commands restated in up to nine sections                              | Mostly fixed by the lean change. What is left is listed in §2.6                                                                                                                          |
| Agent name          | `--as-agent` asked for on `comment add`                                    | Also asked for on `run finish`; the default from `PIDASH_AGENT_NAME` covers both (§6.3)                                                                                                  |
| Freshness           | "Don't re-fetch" everywhere                                                | `run-framing` tells timer-started runs to check for newer comments, which is correct. §6.2 makes that check cheap instead of removing it                                                 |
| Rare procedures     | Only CLI reference text moves to `pidash docs`                             | The lean design left its rarely used procedures inline because `pidash docs` did not exist. Issue C may move them (§6.6)                                                                 |

Open follow-ups from the lean change that touch the same files: NEWPIDASH-338
(the stored hand-off is lost at run completion; four other `finish` and
prompt bugs) and NEWPIDASH-339 (simplifying `finish`). Do NEWPIDASH-338
before issue C, because both edit `lifecycle.md`.

Sections 1 and 2 keep the original evidence from PIDASHCLOU-2. Where the
lean change altered a fact, the text says so.

---

## 1. Why

Test issue PIDASHCLOU-2 ("comment which agent and LLM you are") cost
**~1.66M tokens / $2.98** across three runs (implement, review, test) on
`homepc_claude01` (Claude Code, `claude-opus-4-8`). The same question sent
directly to `claude -p` cost **~25k tokens / $0.15**.

Agents have no memory between steps: every tool call re-sends the whole
context. Cost is roughly

```
cost ≈ (fixed context per step) × (steps per run) × (runs per issue)
```

For PIDASHCLOU-2: ~55k tokens × 7–10 steps × 3 runs. The work itself grew the
context by only ~9k tokens per run, so ~95% of the spend is the fixed context
being re-sent. This design attacks two of those three factors:

| Factor                 | What inflates it today                                                 | Share on PIDASHCLOU-2               |
| ---------------------- | ---------------------------------------------------------------------- | ----------------------------------- |
| Fixed context per step | the 34 KB `pidash-cli` section, ~60% of which a typical run never uses | ~200k tokens removable              |
| Steps per run          | re-fetching inlined data, failed bare commands, manual bookkeeping     | ~7 of 26 steps ≈ 420k tokens (~25%) |

These numbers predate the lean agent workflow change. That change removes
whole runs for simple issues (PIDASHCLOU-2 would now be one In Progress run,
not three) and cut the non-CLI part of each prompt, which leaves the CLI
section as most of what is re-sent on every step (§2.1). It has not been
measured end to end yet; §8 measures both changes together.

Sources: run transcripts under
`~/.local/share/pidash-test/runners/7a9bed71-…/history/runs/` and Claude Code
sessions under `~/.claude/projects/-home-rich--local-share-pidash-test-workspaces-pidashclou-homepc-claude01-7a9bed71/`.

---

## 2. Current state

### 2.1 The `pidash-cli` prompt section (every issue and scheduler run)

`rust-api/crates/services/src/prompting/recipes.rs` puts `"pidash-cli"` in
**every** runner recipe: `coding-task`, `review`, `test`, `scheduler`.

- Source: `rust-api/crates/services/src/prompting/sections/pidash-cli.md`
  (embedded with `include_str!`), ~38 KB template.
- The Django tree (`apps/api/pi_dash/prompting/sections/`) holds an older,
  drifted copy (it still says "An empty file clears it"). Django is out of
  scope, so it is not synced. R10 is resolved by scope, not by a mirror
  check.

Rendered size on PIDASHCLOU-2:

| Stage     | Whole prompt | `## Pi Dash CLI` section | Share |
| --------- | ------------ | ------------------------ | ----- |
| Implement | 96.5 KB      | 34.4 KB                  | 36%   |
| Review    | 68.3 KB      | 34.5 KB                  | 50%   |
| Test      | 76.2 KB      | 34.5 KB                  | 45%   |

After the lean agent workflow change, the rest of the prompt is much
smaller, and the CLI section is unchanged in size (37.7 KB template):

| Stage     | Prompt without the CLI section (measured, PR #3403) | CLI section (rendered, as above) | CLI share (approx.) |
| --------- | --------------------------------------------------- | -------------------------------- | ------------------- |
| Implement | 27.3 KB                                             | ~34 KB                           | ~55%                |
| Review    | 7.9 KB                                              | ~34 KB                           | ~80%                |
| Test      | 8.9 KB                                              | ~34 KB                           | ~80%                |

The first column was measured on a sample issue with the software work
type; a real issue adds its description and comments.

Roughly 8–10k tokens, re-sent on every step. Breakdown (R5):

| Block                                                                                                               | Size    | Typical issue run uses it? |
| ------------------------------------------------------------------------------------------------------------------- | ------- | -------------------------- |
| Schedulers (11 commands, admin-only writes)                                                                         | ~7.5 KB | almost never               |
| Issues: list filters, create, relate/unrelate, relations, search (incl. an old-runner compat note)                  | ~7 KB   | sometimes                  |
| Pods and dedicated runners                                                                                          | ~3.5 KB | rarely                     |
| Pages                                                                                                               | ~3 KB   | rarely                     |
| Labels (+ attach examples)                                                                                          | ~3.5 KB | rarely                     |
| Core: env, output contract, workpad, comment add/list, issue get/patch, run yield, not-for-you, conventions, states | ~9 KB   | always                     |

The flag-level reference is accurate. All 42 documented command paths were
checked against `pidash 0.1.31 --help`, and only one mismatch was found
(§2.4).

### 2.2 The prompt inlines data, then tells the agent to fetch it again (R1)

The composer inlines the issue, comments to date, project states
(`### Available states`) and, for review/test, the workpad body
(`workpad-context.md`). `context.py:650` says the workpad is inlined
specifically to "save an extra `pidash workpad get` round-trip". The CLI
section then undoes that:

- `pidash-cli.md:68`, `comment list`: "Read these in chronological order to
  pick up any human replies since your last run."
- `pidash-cli.md:156`, recipe: "**Always start each edit by re-fetching from
  the server**": `pidash workpad get | jq -r .body > ./.pidash-workpad.md`.
- `test-cycle.md:41`: "read it with `pidash workpad get`."

Even an agent that reads the inlined workpad needs it _as a file_ to edit
it, so it fetches anyway. Redundant or failed calls on PIDASHCLOU-2:

| Run       | Calls                                                                                   |
| --------- | --------------------------------------------------------------------------------------- |
| Implement | bare `workpad get` → error                                                              |
| Review    | `comment list` + `issue get`; bare `state list` + `workpad get` → error; retry with IDs |
| Test      | `comment list`; bare `workpad get` → error; retry with ID                               |

That's about 7 of 26 calls, ~420k tokens.

**Since the lean change.** `workpad-context.md` and `test-cycle.md` are gone,
and review and test no longer use the workpad. What remains:

- the `workpad` section (In Progress only) still says "A successful update
  deletes the file, so before each later edit re-fetch with
  `pidash workpad get | jq -r .body > <path>`", and the CLI section's recipe
  says the same;
- `comment list` in the CLI section still says to read comments "to pick up
  any human replies since your last run";
- `run-framing` tells a timer-started run to run `pidash comment list` for
  anything newer than the prompt. That one is justified, because the prompt
  is built when the run is created and the run may start later; but it
  returns the whole thread again.

### 2.3 The environment contract is wrong (R2)

`pidash-cli.md:10-20` says the CLI reads `PIDASH_API_URL`,
`PIDASH_WORKSPACE_SLUG`, `PIDASH_TOKEN` (a "session-scoped credential"),
`PIDASH_ISSUE_IDENTIFIER` and, for scheduler runs, `PIDASH_PROJECT` from the
environment. In reality:

- The runner sets exactly one: `PIDASH_RUN_ID` (`runner/src/util/shell.rs`
  `apply_run_env`).
- The CLI takes URL, workspace and token from the **runner config file**
  first (`runner/src/api_client.rs` `CliEnv::resolve`). Env vars are a
  fallback.
- The token is the **runner owner's own CLI token** (`api_client.rs:86-91`:
  "the token resolves to the runner owner's account"). It is not
  session-scoped. Agents act with the owner's full permissions.
- `PIDASH_ISSUE_IDENTIFIER` (NEWPIDASH-317) and `PIDASH_PROJECT` are never
  set, so every bare `workpad get` / `workpad update` / `state list` fails.

### 2.4 Scheduler prompt documents a flag that doesn't exist (R3)

For scheduler runs `pidash-cli.md:118` renders
`pidash state list --project <PROJ>`. `state list` has no `--project`. It
takes one positional argument that must be a project **UUID** or an **issue**
identifier (`runner/src/cli/state.rs`). Every scheduler run that follows the
prompt gets a parser error.

### 2.5 Comment attribution is manual (R4)

`pidash-cli.md:69` requires `--as-agent "<name>" --agent-run-id "{{ run.id }}"`
on every comment and asks the agent to guess its runtime name. The CLI
already has the run id (`CliEnv.run_id`, sent as `X-Pi-Dash-Run-Id` on every
write) and the runner knows the agent kind. Neither flag defaults
(`runner/src/cli/comment.rs` `add_speaker_metadata`). A comment posted without
them is attributed to the human owner, and scheduler run tracing
(`apps/api/pi_dash/api/views/scheduler.py:151-171`, keyed on
`speaker_agent_run_id`) silently misses it.

Since the lean change, `pidash run finish` posts the closing comment with
the run id attached by the server, so that comment is always attributed to
the run. The agent's name is still manual: `lifecycle.md` ends the command
with `--as-agent "<your name>"`, and the server falls back to the label
"AI Agent" when it is omitted.

### 2.6 Core commands are re-specified across sections (R7)

Before the lean change, `run yield` outcomes, `--stop-ticking`,
`workpad update`, `issue patch` and the stage-ending flow were each restated
in five to nine sections (the table is in `review.md`, R7). The lean change
removed most of that. What is restated today, besides `pidash-cli` itself:

| Command / rule                       | Also specified in                                                                    |
| ------------------------------------ | ------------------------------------------------------------------------------------ |
| `run finish` syntax and result names | `lifecycle` (the owner) and the CLI section's "Ending the run" entry and two recipes |
| Blocking flow                        | `blocking`, and the CLI section's "Block on a person" recipe                         |
| Workpad edit round-trip              | `workpad` (the owner) and the CLI section's workpad entry and recipe                 |
| `issue patch --state`                | `lifecycle` (old-runner fallback), `analyze-and-scope`, `software/execute`           |
| `comment list`                       | `run-framing`, `analyze-and-scope`, `relationships`, `workpad`                       |
| Retry rule                           | the CLI section's output contract and its conventions list                           |

So the remaining duplication is mostly between the CLI section and the
section that owns each flow.

### 2.7 "Not for you" list is stale (R6)

`pidash-cli.md:152` names `configure` and `rotate`, which no longer exist.
It misses:

- `runner`, including `runner add`, which rewrote and restarted the live
  service when a test ran it on 2026-11-16;
- `update`, `engine`, `reconnect`, `login`, `auth`, `config`;
- `ai`, which calls the cloud AI assistant on the user's BYOK key.

The lean change did not touch this list; it is still as described.

### 2.8 The `pidash` skill in the agent's home directory loads on top of the prompt

The OSS repo ships `skill/pidash/` (`SKILL.md` 9 KB +
`references/pidash-workflows.md` 23 KB), installed by
`npx @airepublic/pidash-skill-installer --claude-code|--codex` into
`~/.claude/skills/pidash` or `~/.codex/skills/pidash`. The private overlay
ships plugin copies (`plugins/claude/pidash`, `plugins/codex/pidash`). It is
written for a person using Pi Dash from their workspace, and its description
("create, read, update… issues; post or read issue comments; inspect workflow
states…") matches every Pi Dash run.

Runners launch agents under the user's home directory, so any installed copy
is visible. Prod runner history on this machine (815 runs):

| Runner                              | Agent       | Runs | Loaded `SKILL.md`                          | Also read `references/pidash-workflows.md` |
| ----------------------------------- | ----------- | ---- | ------------------------------------------ | ------------------------------------------ |
| homepc-homepage-codex001            | codex       | 104  | **95**                                     | **76**                                     |
| homepc-muse-6 … muse-11 (6 runners) | muse_code   | 616  | 416 (59–81% each)                          | 0                                          |
| homepc_pidash_claude_1              | claude_code | 86   | 0 (not installed under `~/.claude/skills`) | 0                                          |

The installed copy here is old (2.6 + 2.4 KB). The current skill is 32 KB, so
anyone installing it today **nearly doubles the CLI cost per step** for
Codex/muse runs, or Claude Code if installed there.

### 2.9 `pidash --help`

Built into the binary and always matches it. Per-command help is accurate
(e.g. `workpad update --help` covers the empty-body guard, `--keep`, the
default identifier). But:

- top-level help lists all ~30 commands, about half of them operator-only;
- the header is outdated ("drives Codex for assigned tasks");
- it lacks field semantics, conventions and recipes.

### 2.10 Per-run values are interleaved with static text (R8)

`{{ run.id }}`, `{{ issue.identifier }}` and `run.kind` branches are spread
through the section. The rendered text diverges per run early, which blocks
moving static guidance into a cross-run-cacheable channel. Each run's first
call writes ~36–45k tokens of prompt cache at the 1-hour rate; only the
~13k-token agent system prefix is shared across runs today.

### 2.11 Nothing checks the prompt against the binary (R9)

§2.3 and §2.4 shipped because no test parses the rendered prompt's commands
with the real CLI or checks the environment variables it promises.

---

## 3. Goals

1. **No redundant fetches.** Data the prompt inlines is usable as-is,
   including the workpad as an editable file. The agent knows when it is
   fresh enough.
2. **The prompt matches reality.** Every command parses; every promised
   environment variable is set; the credential model is described truthfully.
3. **The CLI does its own bookkeeping.** Run id, agent name and issue
   identifier default from the environment, not from prompt instructions.
4. **Each step carries only the CLI knowledge almost every run needs.**
   Everything else is on demand, from one source that matches the installed
   binary, for every agent kind.
5. **One owner per fact.** No command, outcome rule or flow is specified in
   more than one section.
6. **The CLI manual is never in context twice** (skill + prompt).
7. **The operator-command blocklist is complete and generated**, not
   hand-maintained.
8. **All server changes in `rust-api` only**; no Django work.

Non-goal: a prompt so small the agent has to look things up on most runs.
Each lookup is a full extra step (~50–60k tokens re-sent), so a too-small core
costs more than it saves.

---

## 4. Design overview

```
before the agent starts (runner)        prompt (every step)
┌──────────────────────────────┐        ┌────────────────────────────────┐
│ env: PIDASH_RUN_ID           │        │ "This run" block (all per-run  │
│      PIDASH_ISSUE_IDENTIFIER │        │  values in one place)          │
│      PIDASH_PROJECT (sched.) │        │ inlined data + "current as of  │
│      PIDASH_AGENT_NAME       │        │  run start, don't re-fetch"    │
│ file: .pidash/workpad.md     │        │ pidash-cli CORE (~4–6 KB)      │
└──────────────────────────────┘        │  - true env/credential note    │
                                        │  - core commands, exact usage  │
CLI defaults from env                   │  - command index (1 line each) │
┌──────────────────────────────┐        │  - generated never-run list    │
│ issue id, run id, agent name │        │  - conventions                 │
└──────────────────────────────┘        └────────────────────────────────┘
                                                       │ on demand
                                        ┌────────────────────────────────┐
                                        │ pidash docs [<topic>]          │
                                        │ pidash <cmd> --help            │
                                        └────────────────────────────────┘
                                                       ▲ thin pointer
                                        ┌────────────────────────────────┐
                                        │ pidash skill (user-installed)  │
                                        └────────────────────────────────┘
```

The workpad file is written for In Progress (`coding-task`) runs only. The
end-of-run command, its results and the choice of next state stay in the
`lifecycle` section; the core only points at it.

---

## 5. Expected effect

Estimates; §8 replaces them with measurements. The per-step figures assume
about 4 characters per token.

| Change                                               | Mechanism                                                                             | Estimated saving                                              |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| §6.6 core section + `pidash docs`                    | the CLI section goes from ~34 KB to ≤ 6 KB on every step                              | ~7k tokens per step, on every issue and scheduler run         |
| §6.1–6.3 workpad file, cheap freshness, env defaults | no workpad re-fetch before each edit, no failed bare commands, no full-thread re-read | 1–3 fewer steps per In Progress run; fewer on review and test |
| §6.8 thin skill                                      | no second manual on Codex/muse runs                                                   | up to ~8k tokens per step on those runs                       |
| §6.10 per-run block                                  | enables a cacheable static prefix                                                     | cost, not token count (evaluate separately)                   |

What that means per prompt, using the sizes in §2.1:

| Stage     | Prompt today (approx.) | With a ≤ 6 KB core | Reduction |
| --------- | ---------------------- | ------------------ | --------- |
| Implement | ~61 KB                 | ~33 KB             | ~45%      |
| Review    | ~42 KB                 | ~14 KB             | ~65%      |
| Test      | ~43 KB                 | ~15 KB             | ~65%      |

The agent's own system prompt and tool definitions (about 13k tokens on
Claude Code) are re-sent too and are outside this design, so the saving per
step in total context is smaller than these percentages: roughly 7k of
about 28k tokens on an implement step, and 7k of about 23k on a review or
test step.

On PIDASHCLOU-2 the earlier estimate was 1.66M → ~1.1M tokens from this
design alone. With the lean change that issue is one run, not three, so the
absolute saving from this design on it is smaller, and the saving on issues
that do go through review and test is proportionally larger.

## 6. Changes

### 6.1 Run-start materialisation: the workpad as a file (R1)

**Location: `<working_dir>/.pidash/workpad.md`**, not the repo root. The
runner already adds `.pidash/` to `$GIT_DIR/info/exclude`
(`runner/src/cli/context.rs` `exclude_pidash_from_git`), so the file can't be
committed by accident. The current prompt's root-level scratch files
(`./.pidash-workpad.md`, `.pidash-review.md`, `.pidash-comment.md`, …) can be
committed by a careless `git add -A`, and were found left over in a test
workspace. All scratch files move under `.pidash/` (§6.6 recipes, Appendix A).

**Which runs.** Only In Progress (`coding-task`) runs have a workpad since
the lean change: review and test read the hand-off, which the prompt
inlines, and write nothing to the workpad. The snapshot, the file and the
prompt text below apply to `coding-task` runs only. For every other kind
the envelope fields are omitted and the runner writes no workpad file.

**Delivery to the runner.** At run creation, the server snapshots the workpad
into `run_config` (`workpad_body`, `workpad_sha256`). The `Assign` envelope
copies them as additive optional fields, alongside the identifiers from §6.3.
Envelope builder: `rust-api/crates/types/src/runner_sessions/envelopes.rs`
`build_assign_msg`.

Runner side: `runner/src/cloud/protocol.rs` `ServerMsg::Assign` gets the same
fields as `#[serde(default)]` options, so old servers and old daemons stay
compatible.

**Runner.** In `Supervisor::handle_assign`, before the bridge launches the
agent:

- write `workpad_body` to `.pidash/workpad.md` (an empty file if there is no
  workpad);
- write `workpad_sha256` to `.pidash/workpad.sha256`;
- ensure the git exclude.

When the fields are absent (old server), write nothing; the prompt's fallback
line covers it (Appendix A).

**Stale-write guard: content hash, not `updated_at`.** The workpad endpoint's
`updated_at` is the **issue row's** `updated_at` (`IssueWorkpadSerializer`,
`fields = ["body", "updated_at"]`). Any issue edit changes it, including the
agent's own `issue patch --state`, so it would reject legitimate writes. Use
the SHA-256 of the workpad body instead, which needs no migration:

- `GET …/workpad/` adds `body_sha256` to its response.
- `PATCH …/workpad/` accepts an optional `if_body_sha256`. Inside the existing
  `select_for_update` block, if it is present and doesn't match the current
  body's hash, return **412** `{"error": "workpad changed since you read it",
"body_sha256": "<current>"}`. Absent means today's behaviour (unconditional
  write), so older CLIs keep working. On success the response adds
  `body_sha256` of the new body.
- Implement in `rust-api/crates/api/src/v1_work_items/handlers_relations.rs`
  (`get_workpad` / `patch_workpad`), with Rust contract tests.

**CLI.**

- `pidash workpad update`:
  - `--body-file` defaults to `.pidash/workpad.md`;
  - sends `if_body_sha256` from `.pidash/workpad.sha256` when the file exists,
    and on success rewrites that file;
  - on 412 exits non-zero with a JSON error telling the agent to run
    `pidash workpad get`, re-apply its edit, and retry;
  - **keeps the body file by default** (`--keep` accepted as a no-op; new
    `--remove` for the old delete behaviour);
  - keeps the existing empty-body guard (`--allow-empty`).
- `pidash workpad get` gains `--to-file` (default path `.pidash/workpad.md`):
  writes body and hash files and prints `{updated_at, body_sha256, path}`
  instead of the body, so a refresh doesn't put the whole body into the
  agent's context.

**Prompt.** The `workpad` section owns the workpad round-trip (§6.7). Its
last paragraph, which today tells the agent to re-fetch before every edit,
becomes: "Your workpad is in `.pidash/workpad.md` (also shown above). Edit
it, then run `pidash workpad update`; there is no append, so the file is
always the full body. If the file is missing, run
`pidash workpad get --to-file` first. If an update is rejected as changed,
run `pidash workpad get --to-file` and re-apply your edit. Update the
workpad after each meaningful step." The CLI core carries one line that
names the two commands and points at "Workpad" (Appendix A). `workpad` is an
`overridable` section; see §9 for existing overrides.

The same directory holds the files for the closing command: the prompt's
examples use `.pidash/comment.md` and `.pidash/handoff.md` for
`pidash run finish --comment-file … --handoff-file …`.

### 6.2 Say what is fresh, and make the one needed re-read cheap (R1)

The prompt is built when the run is created, and a run can start later
(queued behind another run, or waiting for a runner). So "never re-fetch"
would be wrong, and `run-framing` already tells a timer-started run to look
for newer comments. The change is to make that check small and to stop
every other re-read.

- New context key `run.rendered_at` (UTC RFC 3339, set when the prompt is
  built), added in `rust-api/crates/services/src/prompting/context.rs`.
- New CLI flag `pidash comment list <issue> --since <RFC 3339>`: prints only
  comments created after that time, in the same JSON shape. Filter in the
  CLI after the fetch if the list endpoint has no such filter; the point is
  what reaches the agent's context, not the request size. With nothing new
  it prints an empty list.
- `run-framing` (the owner of this instruction) changes its timer-run
  sentence from "run `pidash comment list {{ issue.identifier }}` for
  anything newer" to
  "run `pidash comment list --since {{ run.rendered_at }}` for anything
  newer". On an old runner the flag is unknown; the fallback is the plain
  `comment list` (Appendix A).
- The CLI core says once: "The issue, comments, states{% if coding-task %}
  and workpad{% endif %}{% if review or test %} and hand-off{% endif %}
  in this prompt were read at `{{ run.rendered_at }}`. Don't fetch them
  again, except: `comment list --since` when "How this run works" tells you
  to or you have worked long enough that a person may have replied; and
  after a write is rejected as stale."
- Remove the "read these… to pick up any human replies" instruction from
  `comment list`. Keep `comment list` / `issue get` / `state list` in the
  command index for genuine refreshes.
- `Available states` stays inlined; the `state list` line points at it.

### 6.3 Runner exports the context the CLI needs; the CLI defaults from it (R2, R3, R4)

**Where the values come from.** The runner doesn't know the issue or project
today: `ServerMsg::Assign` carries only `work_item_id` (UUID). The server adds
two additive optional fields to `Assign`:

- `issue_identifier` (e.g. `PIDASHCLOU-2`), for issue runs;
- `project_identifier` (e.g. `PIDASHCLOU`), for issue and scheduler runs.

Both are filled from `run_config`, set at run creation in
`rust-api/crates/services/src/orchestration/creation.rs` and the scheduler
run path (`rust-api/crates/services/src/scheduler/`). The envelope builder is
the same as §6.1.
The agent name comes from runner config: `AgentKind::display_name()`
(`runner/src/config/schema.rs`) plus the effective model (`Assign`'s
`expected_codex_model`, else the runner's `model_default`, else omitted).

Runner (`runner/src/util/shell.rs` `RunEnv` / `apply_run_env`; `RunEnv` is
constructed in `Supervisor::handle_assign`, `runner/src/daemon/supervisor.rs`).
Values are also re-asserted after the login shell's rc files:

| Variable                  | Issue runs                        | Scheduler runs | Chat |
| ------------------------- | --------------------------------- | -------------- | ---- |
| `PIDASH_RUN_ID`           | ✓ (today)                         | ✓ (today)      | –    |
| `PIDASH_ISSUE_IDENTIFIER` | ✓ (NEWPIDASH-317)                 | –              | –    |
| `PIDASH_PROJECT`          | ✓                                 | ✓              | –    |
| `PIDASH_AGENT_NAME`       | ✓ `Claude Code (claude-opus-4-8)` | ✓              | –    |

When a field is absent (old server), the variable is not set, and the CLI
behaves as today.

CLI:

- `workpad get|update`, `state list` default the issue from
  `PIDASH_ISSUE_IDENTIFIER` (already implemented; works once exported).
- `state list` gains `--project <PROJ-or-UUID>` accepting project
  identifiers like every other project-scoped command, and defaults it from
  `PIDASH_PROJECT`. This fixes the scheduler prompt (§2.4).
- `comment add` defaults `--as-agent` from `PIDASH_AGENT_NAME` and
  `--agent-run-id` from `PIDASH_RUN_ID` when they are set and the flags are
  absent (`runner/src/cli/comment.rs` `add_speaker_metadata`). Explicit flags
  win. Server-side inference from `X-Pi-Dash-Run-Id` is deferred (§10, D1).
- `run finish` defaults `--as-agent` from `PIDASH_AGENT_NAME` the same way
  (`runner/src/cli/run_cmd.rs` `FinishArgs`). Today the lifecycle prompt
  asks the agent to type its own name, and the server labels the comment
  "AI Agent" when it doesn't.
- `comment list` gains `--since` (§6.2).

Prompt: core commands are shown in their short form
(`pidash comment add <id> --body-file <path> [--fold]`), the explicit
`--as-agent` / `--agent-run-id` instructions are removed from the CLI
section, and `--as-agent "<your name>"` is removed from the `run finish`
line in `lifecycle.md`. On a runner older than this change, a bare
`comment add` still succeeds but is attributed to the owner, and a
`run finish` comment is labelled "AI Agent". That's acceptable for the few
runners that lag, and is why the prompt change ships only after the runner
release and the `MIN_RUNNER_VERSION` bump (§7).

### 6.4 Truthful environment and credential section (R2)

Replace `pidash-cli.md:10-20` with:

> `pidash` is already authenticated **as the runner's owner**; your writes
> carry their identity plus this run's id. Never pass credentials as flags,
> and never print anything from `PIDASH_*` variables or the pidash config.
> `PIDASH_ISSUE_IDENTIFIER`, `PIDASH_PROJECT` and `PIDASH_RUN_ID` are set for
> you, so commands default to this issue and run.

Drop `PIDASH_API_URL`, `PIDASH_WORKSPACE_SLUG` and `PIDASH_TOKEN` from the
prompt; the agent never needs them.

### 6.5 Generated operator-command blocklist (R6)

- In `runner/src/cli/mod.rs`, classify every top-level subcommand as
  `agent` or `operator`. A unit test fails if a new subcommand is unclassified.
- `pidash --help` inside a run (`PIDASH_RUN_ID` set) hides operator commands
  and says so. Fix the outdated "drives Codex" header.
- Inside a run, operator commands refuse with a clear error (exit code +
  JSON `{"error": "operator command not available inside an agent run"}`).
  An escape hatch for genuine operator debugging inside a shell that inherited
  the variable: `PIDASH_ALLOW_OPERATOR=1`.
- `ai` is classified operator (it spends the user's BYOK key).
- The prompt's "Not for you" line becomes one sentence: "Commands that manage
  the runner itself are refused inside a run; if one seems necessary, the run
  is blocked." Refusal is enforced by the CLI, so the prompt doesn't need to
  enumerate them. `issue re-tick` stays named (it is a human decision, refused
  server-side).
- Since the lean change the server also refuses an issue run that moves its
  own issue to Done, Cancelled or a stage the project switched off, whether
  through `run finish` or `issue patch --state`. `lifecycle` states the rule;
  the CLI reference (`pidash docs issue`) repeats it next to
  `issue patch --state` so an agent that reaches for that command sees it.

### 6.6 Core section + `pidash docs` (R5)

Rewrite `sections/pidash-cli.md` **in place** (same key; it is a `locked`
section, so no workspace override exists) to contain only:

1. The environment/credential note (§6.4) and the output contract (one
   paragraph; the retry rule lives here only).
2. The freshness note (§6.2).
3. **Core commands, exact usage:**
   - issue runs: `comment add` (a question or note during the run), the
     workpad line for `coding-task` runs (§6.1), `issue get` and
     `comment list [--since]` as refresh commands, and one line for
     `run finish` that gives no syntax and points at "Lifecycle", which
     owns it (§6.7);
   - scheduler runs: `issue create` for filing a finding, `comment add`, and
     `run yield` (optional, last call). A scheduled run has no stages and
     does not use `run finish`.
4. **Command index**, one line per remaining group, e.g.

   ```
   Other commands (run `pidash docs <topic>` or `pidash <cmd> --help` before first use):
     issue      create, patch (title, labels, state of another issue), list, search,
                relate/unrelate/wait, relations, run-ai, --pod/--runner
     page       project wiki pages: list, get, create, update, archive
     label      list, create, update, delete
     scheduler  view/update project schedulers
     project    runners
     context    the context.md files that list this machine's projects
   ```

5. The blocklist sentence (§6.5).
6. **Conventions:** no undo, token secrecy, fenced JSON.

`issue patch --state` leaves the core for issue runs: a run moves its own
issue with `run finish`, and moving another issue is rare. It stays in the
index and in `lifecycle`'s old-runner fallback.

Scheduler runs get the scheduler commands as their core (the template
already branches on `run.kind`). Issue runs see schedulers only as one
index line.

Removed from the prompt entirely:

- the `issue search` backwards-compat note (the runner release this depends
  on is newer than `search`, §7);
- the "Ending the run" entry and the three end-of-run recipes (finish a
  task, finish a review or test pass, block on a person): `lifecycle` and
  `blocking` own those (§6.7);
- the workpad recipe (replaced by §6.1);
- the "Working context files (`context.md`)" block (~1.3 KB), which explains
  cross-project lookups that few runs need. It becomes the `context` topic
  of `pidash docs`, with one index line.

Add `pidash docs [<topic>]` to the runner CLI:

- The content is a markdown reference (`runner/docs/cli-reference.md`,
  embedded with `include_str!`) holding everything removed from the prompt:
  full flag descriptions, output fields (`relations_summary`,
  `has_open_blockers`, `dispatch_wait`, …), pods/runners policy,
  page/label/scheduler details, the `context.md` files, the remaining
  recipes.
- `pidash docs` lists topics; `pidash docs <topic>` prints one (`issue`,
  `comment`, `workpad`, `page`, `label`, `state`, `scheduler`, `pods`, `run`,
  `context`, `recipes`). Plain markdown on stdout, so `pidash docs | grep …`
  works.
- The `run` topic documents `run finish` in full (flags, results, what the
  server refuses, that a repeated call changes nothing) and `run yield` for
  scheduler runs.
- Static content only. Anything run-specific stays in the prompt.

**Rare procedures from other sections (optional, in issue C).** The lean
design kept two rarely used procedures inline because `pidash docs` did not
exist: the split-into-sub-issues procedure in `analyze-and-scope` and the
comment examples in `software/execute`. Once `pidash docs` ships, each may
move to a `recipes` sub-topic with a one-line pointer left behind. Move one
only if the section's rule stays in the prompt and only the worked
procedure moves, and check §8's lookup metric afterwards. This is a
follow-on saving, not a requirement of this design.

Why not a markdown file next to the binary: same content, but adds an install
path the agent must be told and a way to drift. Why not only `--help`: help
text is the wrong place for recipes and field semantics. `--help` stays the
place for flags (and gains `--as-agent`-style details in `long_about`).

**Size tests.** The lean change added a rendered-size test per recipe kind
that excludes the CLI section (budgets 50 / 12 / 13 KB). Add:

- the rendered CLI section ≤ 6 KB for each kind;
- the whole rendered prompt on the sample issue ≤ 56 / 18 / 19 KB for
  `coding-task` / `review` / `test`.

### 6.7 One owner per fact (R7)

The lean change set the owners for the run's flow. This design removes the
second copies that the CLI section still holds, and takes ownership of
command syntax for everything else.

| Fact                                                          | Owner                            | Everyone else                                            |
| ------------------------------------------------------------- | -------------------------------- | -------------------------------------------------------- |
| `run finish`: syntax, result meanings, which state to move to | `lifecycle`                      | the CLI core names the command and points at "Lifecycle" |
| Asking a person, blocking                                     | `blocking`                       | refer to it                                              |
| Workpad content and edit round-trip                           | `workpad` (In Progress only)     | the CLI core names the two commands                      |
| When to look for newer comments                               | `run-framing`                    | the CLI core's freshness note refers to it               |
| Syntax and one-line semantics of every other command          | `pidash-cli` core, `pidash docs` | refer to the command by name                             |
| Retry rule                                                    | output contract in `pidash-cli`  | –                                                        |

Concretely:

- delete from `pidash-cli.md` the "Ending the run" entry, the three
  end-of-run recipes, the workpad recipe and the duplicate retry rule in the
  conventions list;
- in `analyze-and-scope`, `relationships`, `workpad` and `software/execute`,
  keep the instruction to use a command but drop restated flags, since the
  core or `pidash docs` has them;
- once the runner release with `run finish` is the minimum (§7), delete the
  old-runner fallback sentence from `lifecycle.md` (about 400 characters on
  every issue run). That removes the last `run yield`, `--stop-ticking` and
  `issue patch --state` text from issue-run prompts.

Keep one sentence of emphasis where a past failure justifies it, but no
second copy of a specification.

### 6.8 Thin skill, kept out of Pi Dash runs

The skill stays (it is useful to humans driving Pi Dash from their own
workspace) but stops being a second copy of the manual:

- `SKILL.md` becomes short: what Pi Dash is, the core commands, "run
  `pidash docs <topic>` for the full reference". Drop
  `references/pidash-workflows.md`; its content moves to `pidash docs` (one
  source for humans and runs).
- First line of the skill body: **"If `PIDASH_RUN_ID` is set you are inside a
  Pi Dash agent run: your prompt already contains the CLI instructions. Do not
  load further references; use `pidash docs` only for commands not covered
  there."**
- The core prompt section adds: "You don't need to load a `pidash` /
  `pi-dash` skill; this section is the reference."
- Apply to the OSS `skill/pidash/` and the overlay `plugins/{claude,codex}/pidash/`
  copies (or make the overlay consume the OSS copy).

We can't stop an agent from loading a skill installed in the user's home
directory, and we don't change the user's agent config. So the skill is made
cheap when loaded, and the agent is told it's unnecessary.

### 6.9 Prompt-vs-CLI contract test (R9)

Runner-side test that:

1. renders each recipe kind (`coding-task`, `review`, `test`, `scheduler`)
   from fixtures exported by the Rust composer tests, with stage settings
   that cover Auto and a switched-off stage;
2. extracts every `pidash …` command line and parses it with the real clap
   `Command` (`try_get_matches_from`), substituting placeholders;
3. asserts every `PIDASH_*` variable the prompt promises is set by
   `apply_run_env` for that run kind;
4. asserts the never-run sentence and the generated classification (§6.5)
   agree;
5. asserts every quoted section reference (`see "…"`) matches a heading
   that is rendered for that kind. Today eight references say "Lifecycle",
   while the heading in that section's body is "Where this task is, and how
   the run ends". "Lifecycle" is the section's title in its front matter,
   which the composer keeps as metadata; confirm it is not rendered, then
   fix the references or the heading;
6. covers the whole prompt, not only the CLI section: the `run finish` line
   in `lifecycle`, `--handoff-file` appearing only for `coding-task`, the
   `comment list --since` line in `run-framing`, and every command in
   `blocking`, `workpad` and the work-type slots.

Fixtures come from the Rust composer
(`rust-api/crates/services/src/prompting/`), regenerated by a test there that
fails when they are out of date. There is no Django/Rust mirror check;
Django is out of scope.

### 6.10 Per-run values in one block (R8)

- Collect per-run values at the top of the prompt in a short "This run"
  block: issue identifier, run id, stage, agent name, rendered-at time.
- Static sections refer to "this issue" / "this run" instead of templating
  ids inline. §6.3 already removes most `{{ run.id }}` uses.
- The lean change added per-run values to `lifecycle` (the project's path,
  the current state, the run budget) and `run-framing` (what started the
  run). Those are short and belong where they are; for the caching
  follow-up they would need to move into the "This run" block too, which is
  part of that evaluation, not of this design.
- This makes the static sections identical across runs of the same kind,
  which (a) simplifies testing and (b) enables a follow-up: delivering static
  guidance through a cacheable channel (e.g. Claude Code
  `--append-system-prompt`, Codex instructions), where it would be read from
  cache instead of written fresh on every run's first call. That follow-up
  touches each agent bridge (`runner/src/bridges/*`) and is evaluated
  separately.

---

## 7. Rollout

Ordered so each step is independently shippable and the cheap, high-value
fixes go first.

| #   | Change                                                                                                                                                                                                                                  | Where                  | Issue |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- | ----- |
| 1   | Runner context + workpad file: `Assign` identifiers and workpad snapshot, env exports, CLI defaults (`state list --project`, `comment add`, `run finish`), `comment list --since`, `body_sha256` guard, `.pidash/workpad.md` (§6.1–6.3) | rust-api, runner       | A     |
| 2   | Agent-facing CLI: operator classification and refusal, agent-only `--help`, `pidash docs`, thin skill (§6.5, 6.6, 6.8)                                                                                                                  | runner, skill          | B     |
| 3   | Prompt rewrite + prompt-vs-CLI contract test + measurement (§6.2, 6.4, 6.6, 6.7, 6.9, 6.10, Appendix A)                                                                                                                                 | rust-api, runner tests | C     |

A and B can proceed in parallel. C lands after both.

**No flag.** The earlier revision put the new prompt behind
`PROMPT_PIDASH_CLI_V2`, as a second section key chosen in `recipe_for()`.
That is withdrawn, for the reason the lean change gave: one prompt, replaced
outright, with the old one gone. `pidash-cli` is `locked`, so there is no
workspace override to protect either.

The composer still can't gate on the runner's version: the prompt is
rendered at run creation
(`rust-api/crates/services/src/orchestration/creation.rs`), before the pod
matcher picks a runner. So the order of releases does the gating:

1. **Server side of issue A** deploys first. Its changes are additive
   (optional envelope fields, an optional request field on the workpad
   endpoint), so old runners are unaffected.
2. **A runner release** containing issues A and B, which also carries
   `pidash run finish` from the lean change if no earlier release did.
3. **`MIN_RUNNER_VERSION` is raised** to that release. That setting is
   advisory: old runners show a red banner and keep working.
4. **Issue C merges and deploys** once the operator's runners are upgraded.
   For the hosted cloud, wait one release cycle after the bump. Self-hosters
   who deploy the server before upgrading runners get the fallbacks below;
   the release notes say to upgrade runners first.
5. **Fallbacks in the prompt** for an old runner (Appendix A), each one
   short:
   - workpad file missing → `pidash workpad get --to-file`, and if that flag
     is unknown, `pidash workpad get | jq -r .body > .pidash/workpad.md`;
   - `pidash docs` unknown → `pidash <cmd> --help`;
   - `comment list --since` unknown → `pidash comment list`;
   - bare commands failing for lack of `PIDASH_ISSUE_IDENTIFIER` → pass
     `{{ issue.identifier }}`.

   Each costs one extra step on old runners only, never a dead end.

6. **One release cycle later**, remove the fallbacks and the `run finish`
   fallback in `lifecycle.md` (§6.7).

Rolling back is a revert of issue C's commit; nothing in A or B depends on
the prompt.

## 8. Measurement

Three points on the test runner, so the two designs can be told apart: the
PIDASHCLOU-2 baseline (1.66M tokens, before either change), after the lean
agent workflow change (its §10.3 measurement, still to be run), and after
this one. Same issue shapes each time: a PIDASHCLOU-2-style answer-only
issue and one small real code issue that goes through review, for
`claude_code`, `codex` and `muse_code` runners:

- first-call context tokens; rendered prompt size per kind (compiled-prompt
  preview endpoint);
- steps per run and total tokens per run, per issue;
- **redundant fetches per run**: `workpad get` / `issue get` /
  `state list` calls, and `comment list` without `--since`, with no
  intervening reason. Target ~0 on the answer-only issue;
- failed `pidash` calls per run. Target 0 on both issues;
- `pidash docs` / `--help` lookups per run (the core is too small if this
  exceeds ~0.5 per run on typical issues);
- skill loads per run (target: no `references/` reads; `SKILL.md` loads are
  cheap);
- comments with missing `speaker_agent_run_id` from agent runs, and closing
  comments labelled "AI Agent" instead of the agent's name. Target 0.

---

## 9. Risks

- **The agent misses something it used to have inline** and either guesses
  flags or adds lookup steps. Mitigation: the index, `--help` on every
  command, the §8 lookup metric, and moving a command back into the core if
  lookups for it are frequent.
- **Stale data after the freshness change.** An agent that no longer re-reads
  comments may miss a human reply posted mid-run. Mitigation: the freshness
  note's explicit exceptions (long runs, before irreversible moves); runs are
  usually minutes long; the next tick sees the reply anyway.
- **Workpad overwrite race** once the file is kept. Mitigation: the
  `body_sha256` precondition (§6.1). The agent re-fetches only on 412.
- **Refusing operator commands breaks a legitimate workflow** run from a
  shell that inherited `PIDASH_RUN_ID`. Mitigation: the
  `PIDASH_ALLOW_OPERATOR=1` escape hatch, and a clear error message.
- **Overridden sections.** `pidash-cli`, `lifecycle` and `run-framing` are
  `locked`, so they have no overrides. `workpad` is `overridable`, and its
  round-trip paragraph changes (§6.1). An existing override keeps the old
  re-fetch recipe, which still works on a new runner but wastes a step. Add
  the old recipe's text (`pidash workpad get | jq`) to the retired markers
  that `revalidate_section_overrides` looks for, so such overrides are
  flagged for review, and run it after deploying issue C.
- **No flag to flip back.** A bad prompt change is reverted by reverting the
  commit and redeploying, which is slower than a flag. Mitigation: the
  contract test (§6.9), the size tests, and measuring on the test server
  before the production deploy.
- **Both designs' follow-ups edit the same files.** NEWPIDASH-338 and
  NEWPIDASH-339 touch `lifecycle.md` and the `finish` path. Land
  NEWPIDASH-338 before issue C.
- **The skill is still loaded by Codex/muse.** Made cheap, not prevented. If
  §8 shows the instruction doesn't stop loads, consider a per-agent mechanism
  to skip that one skill in runs (touches user config; deferred).

---

## 10. Decisions (former open questions)

- **D1. Comment attribution:** CLI defaults only (§6.3). Server-side
  inference from `X-Pi-Dash-Run-Id` is deferred. It changes the comment API
  contract and is not needed once the CLI defaults ship.
  Revisit if §8 still shows unattributed agent comments.
- **D2. Workpad stale guard:** the endpoint has no precondition today, and
  its `updated_at` is the issue row's, which the agent's own edits change. Use
  `body_sha256` / `if_body_sha256` (§6.1). This is an additive API change in
  `rust-api`, with no migration.
- **D3. Gating (revised 2026-11-20):** no flag and no second section key.
  `pidash-cli` is rewritten in place and ships after the runner release and
  the `MIN_RUNNER_VERSION` bump, with in-prompt fallbacks for old runners
  (§7). The composer has no runner at render time, so release order is the
  gate. This follows the lean agent workflow decision to replace prompts
  outright.
- **D4. Skill sources:** the OSS `skill/pidash/` is the single source. The
  private overlay's `plugins/{claude,codex}/pidash/skills/pidash/` become thin
  copies generated from it, or are deleted. That change lives in the private
  repo and is tracked separately (a follow-up filed from issue B).
- **D5. `PIDASH_AGENT_NAME` format:** `<display name> (<model>)`, e.g.
  `Claude Code (claude-opus-4-8)`, or just `<display name>` when the model is
  unknown. The display name comes from `AgentKind::display_name()`.
- **D6. Scratch files:** all agent scratch files live under `.pidash/`, which
  is git-excluded, not in the repo root (§6.1).
- **D8. Server scope is Rust only.** Django (`apps/api/`) gets no changes,
  including no prompt-section sync.
- **D7 (withdrawn 2026-11-20).** It said the workpad stays inlined in review
  and test prompts. Since the lean change those runs do not use the workpad;
  they get the hand-off inline. The workpad is inlined, and written to file,
  for In Progress runs only.
- **D9. End of run:** this design does not specify it. `pidash run finish`
  and the `lifecycle` section own it; the CLI core only names the command.
  `run yield` is documented for scheduler runs.
- **D10. Newer comments:** timer-started runs keep checking for comments
  newer than the prompt, through `comment list --since {{ run.rendered_at }}`
  (§6.2), so the check costs a few lines, not the whole thread.
- **D11. `pidash docs` may take rare procedures from other sections**
  (§6.6), decided per procedure in issue C and checked against the lookup
  metric.

---

## 11. Implementation plan (three issues)

Tracking issue: NEWPIDASH-318. Issues: A = NEWPIDASH-320, B = NEWPIDASH-321,
C = NEWPIDASH-325. Server work is in `rust-api` only. Their descriptions
predate this revision; where they differ, this document is the
specification.

### Issue A: runner context and the workpad file (§6.1, §6.2, §6.3)

Implements NEWPIDASH-317.

- `rust-api`:
  - At run creation (`services/src/orchestration/creation.rs`, scheduler run
    path), put `issue_identifier` and `project_identifier` into `run_config`
    for every run, and `workpad_body` and `workpad_sha256` for `coding-task`
    runs only.
  - Copy them into the `assign` envelope
    (`types/src/runner_sessions/envelopes.rs` `build_assign_msg`).
  - Workpad endpoint (`api/src/v1_work_items/handlers_relations.rs`): add
    `body_sha256` to GET/PATCH responses, and accept an optional
    `if_body_sha256` that returns 412 on mismatch.
- `runner`:
  - Add the four fields to `ServerMsg::Assign` as `#[serde(default)]`.
  - In `Supervisor::handle_assign`, when the workpad fields are present,
    write `.pidash/workpad.md` and `.pidash/workpad.sha256`; always ensure
    the git exclude.
  - Extend `RunEnv` / `apply_run_env` with `PIDASH_ISSUE_IDENTIFIER`,
    `PIDASH_PROJECT` and `PIDASH_AGENT_NAME`.
  - CLI:
    - `state list --project` plus env defaults;
    - `comment add` and `run finish` default `--as-agent` from
      `PIDASH_AGENT_NAME`;
    - `comment list --since <RFC 3339>`;
    - `workpad update` default path, hash precondition, keep by default;
    - `workpad get --to-file`.

Done when: on the test runner, a bare `pidash workpad get`,
`pidash workpad update` and `pidash state list` succeed inside a
`coding-task` run; a review run has no `.pidash/workpad.md`; a
`run finish` comment posted with no `--as-agent` carries the agent's name;
`comment list --since` with a time after the last comment prints an empty
list; an update against a changed workpad returns 412.

### Issue B: agent-facing CLI (§6.5, §6.6, §6.8)

- `runner/src/cli/mod.rs`: classify every subcommand as agent or operator,
  refuse operator commands inside runs (`PIDASH_ALLOW_OPERATOR=1` bypass),
  hide them from `--help` inside runs, fix the header.
- `pidash docs [<topic>]` from `runner/docs/cli-reference.md`
  (`include_str!`), with the content moved out of `pidash-cli.md`, including
  the `run` topic (`run finish` in full, `run yield` for scheduler runs) and
  the `context` topic.
- Thin `skill/pidash/` (≤ 2 KB, no references directory), a new installer
  release, and a follow-up issue for the private overlay's plugin copies (D4).

Done when: inside a run, `pidash runner add` exits non-zero with the
refusal and `pidash --help` lists no operator command; `pidash docs` lists
the topics and each prints; a unit test fails on an unclassified
subcommand; the installed skill is ≤ 2 KB.

### Issue C: prompt rewrite, contract test, rollout (§6.2, 6.4, 6.6, 6.7, 6.9, 6.10, §7, Appendix A)

Blocked by A, B and NEWPIDASH-338.

- `rust-api/crates/services/src/prompting/`:
  - rewrite `sections/pidash-cli.md` in place from Appendix A (no flag, no
    new key);
  - `run.rendered_at` in `context.rs`;
  - `run-framing.md`: `comment list --since {{ run.rendered_at }}`;
  - `workpad.md`: the `.pidash/workpad.md` round-trip (§6.1);
  - `lifecycle.md`: drop `--as-agent "<your name>"` from the `run finish`
    line; use `.pidash/` paths in examples;
  - "This run" block and the remaining one-owner cleanup (§6.7);
  - size tests (§6.6); regenerate `rust-api/fixtures/prompting/`;
  - add the old workpad recipe to the retired markers in
    `revalidate_section_overrides` (§9).
- Runner-side prompt-vs-CLI contract test over Rust-rendered fixtures (§6.9),
  and over `pidash docs` output.
- Optional: move the split procedure and the comment examples to
  `pidash docs recipes` (§6.6).
- Rollout and measurement (§7, §8) on the test runner, with numbers posted on
  the issue.
- One release cycle after the production deploy: remove the old-runner
  fallbacks, including the `run finish` fallback in `lifecycle.md`.
- Follow-up investigation, after measurement: deliver static guidance through
  a cross-run cacheable channel per agent bridge (§6.10). This produces a
  short note; implementation is only filed if it's worth it.

Done when: the contract test runs in CI and fails on a prompt command the
CLI cannot parse (shown once with a temporary bad fixture); the size tests
pass; `PROMPT_PIDASH_CLI_V2` and `pidash-cli-core` appear nowhere in the
code; on the test runner, for the two issue shapes in §8, there are zero
failed `pidash` calls, about zero redundant fetches, and the per-step
context is down by the amounts in §5 within 20%; the numbers are posted on
the issue.

## Appendix A — Draft `pidash-cli` section

This replaces `sections/pidash-cli.md` in place. Target ≤ 6 KB rendered for
every kind. The end-of-run command, its results and the choice of state are
owned by `lifecycle` (rendered heading "Where this task is, and how the run
ends"); the workpad by `workpad`; blocking by `blocking`. Section references
use the headings the agent actually sees.

````markdown
## Pi Dash CLI (`pidash`)

`pidash` is your only channel to Pi Dash. It is already authenticated **as
this runner's owner**; your writes carry their identity plus this run's id.
Do not use `curl` or raw HTTP. Never print, log or record any `PIDASH_*`
value or anything from the pidash config.

{% if run.kind != "scheduler" %}`PIDASH_ISSUE_IDENTIFIER`, `PIDASH_PROJECT`
and `PIDASH_RUN_ID` are set, so commands default to this issue and run. (If a
bare command says the issue identifier is missing, pass `{{ issue.identifier }}`.)
{% else %}`PIDASH_PROJECT` (`{{ project.identifier }}`) and `PIDASH_RUN_ID`
are set. There is no current issue: pass identifiers explicitly.
{% endif %}
Every command prints one JSON document on stdout and exits 0. On failure it
prints `{"error": …}` on stderr and exits non-zero: read it, retry only
transient failures, and never retry the same command more than twice.

### What you already have

{% if run.kind == "scheduler" %}The project and its states shown in this
prompt were read at `{{ run.rendered_at }}`.{% else %}The issue, its
comments, the available states{% if run.kind == "coding-task" %} and your
workpad{% else %} and the hand-off{% endif %} shown in this prompt were read
at `{{ run.rendered_at }}`. Don't fetch them again. Two exceptions: check
for newer comments with `pidash comment list --since {{ run.rendered_at }}`
when "How this run works" tells you to, or when you have worked long enough
that a person may have replied; and re-read after a write is rejected as
stale. (If `--since` is not a known flag, use `pidash comment list`.){% endif %}

### Core commands

{% if run.kind != "scheduler" %}- `pidash comment add --body-file .pidash/<name>.md [--fold]`: post a
comment during the run, as this agent run. Your closing comment goes
through `pidash run finish`, not this. Use `--fold` only for low-value
status notes.
{% if run.kind == "coding-task" %}- `pidash workpad update` / `pidash workpad get --to-file`: write and
re-read `.pidash/workpad.md`. See "Workpad".
{% endif %}- `pidash issue get` / `pidash comment list [--since <time>]`: refresh only
(see above).

- `pidash run finish …`: ends the run; your **last** `pidash` call. The
  syntax, the results and the state to pass are in "Where this task is, and
  how the run ends".
  {% else %}- `pidash issue create --project {{ project.identifier }} --title "<summary>" --description-file .pidash/finding.md`:
  file a finding.
- `pidash comment add <issue> --body-file .pidash/<name>.md [--fold]`: post a
  comment as this agent run.
- `pidash run yield --outcome <progressed|waiting_on_human|waiting_on_external|done|blocked> [--note "<one line>"]`:
  optional; if sent, your last `pidash` call. See "Ending the scheduled run".
  {% endif %}
  Keep every scratch file under `.pidash/`; it is never committed.

### Other commands

Run `pidash docs <topic>` (or `pidash <command> --help`) before first use.
If `pidash docs` is not a known command, use `--help`.

- `issue`: create, patch (title, description, labels, another issue's
  state), list/filter, search history, relate/unrelate/wait, relations, pods
  and dedicated runners, run-ai
- `page`: project wiki pages (read before re-deriving a convention)
- `label`: list, create, update, delete; attach via `issue patch --add-label`
- `state`: list workflow states (already shown below)
- `scheduler`: view or change scheduled automation (admin; only when asked)
- `project`: list runners
- `context`: the `context.md` files that list the projects on this machine

You don't need to load a `pidash` / `pi-dash` skill: this section and
`pidash docs` are the reference. Commands that manage the runner itself are
refused inside a run; if one seems necessary, the run is blocked (see
"Blocking the run"). `pidash issue re-tick` is a human decision; never call it.

### Available states

{# rendered list, unchanged from today #}

### Conventions

- All writes are real and immediate; there is no undo. Check intent against
  {% if run.kind == "coding-task" %}your workpad plan{% else %}the issue and
  its comments{% endif %} before mutating.
- When pasting `pidash` JSON into a comment{% if run.kind == "coding-task" %}
  or the workpad{% endif %}, fence it in ` ``` `.
````

Changes to other sections that go with it (issue C):

- `lifecycle.md`, the closing command:

  ```
  pidash run finish --to "<state>" --result <result> --comment-file .pidash/comment.md{% if run.kind == "coding-task" %} [--handoff-file .pidash/handoff.md]{% endif %}
  ```

  (`--as-agent` removed; the old-runner fallback sentence stays until §7
  step 6.)

- `run-framing.md`, the timer-run sentence: "…the comments above were
  captured when the run was created, so run
  `pidash comment list --since {{ run.rendered_at }}` for anything newer and
  address a new human comment first."
- `workpad.md`, the last paragraph: the text in §6.1.
- Every reference to "Lifecycle" in `pidash-cli.md`, `default-posture.md`
  and `implementation.md` becomes the rendered heading, or the heading in
  `lifecycle.md` is renamed to "Lifecycle"; pick one and let the contract
  test hold it (§6.9).
