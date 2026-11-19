# Optimize how the `pidash` CLI reaches the agent

> Directory: `.ai_design/optimize_pidash_cli_prompt/`
>
> **Status:** implementation-ready (reviewed 2026-11-19). Not started.
> Decisions are in §10. Work breakdown (three issues, server side in Rust only) is in §11,
> and a draft of the new core prompt section is in Appendix A.
>
> **Companion:** `review.md`, a line-by-line review of the current
> `pidash-cli` section with the evidence behind each change here (findings
> R1–R10).
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
> - Routing trivial issues through fewer stages (In Progress only vs. In
>   Progress → In Review → In Test). That is a separate design.

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

### 2.6 Core commands are re-specified across sections (R7)

| Command / rule          | Sections that restate it besides `pidash-cli`                                                                                           |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `run yield` outcomes    | ending-run (fullest), blocking, analyze-and-scope, relationships, task-lifecycle, implementation                                        |
| `--stop-ticking`        | ending-run, blocking, analyze-and-scope, review-cycle, session-framing, test-cycle, task-lifecycle                                      |
| `workpad update`        | blocking, default-posture, workpad-setup, ending-run, guardrails, workpad-template, implementation, workpad-context, `software/execute` |
| `issue patch`           | blocking, analyze-and-scope, autonomy, ending-run, implementation, task-lifecycle, `software/execute`                                   |
| `issue relate` / `wait` | blocking, analyze-and-scope, relationships, session-framing, ending-run, workpad-template, task-lifecycle                               |
| Stage-ending flow       | `pidash-cli.md:164-193` recipes, the cycle section's Step 3, ending-run (three copies)                                                  |
| Retry rule              | `pidash-cli.md:32` and `:216`                                                                                                           |

### 2.7 "Not for you" list is stale (R6)

`pidash-cli.md:152` names `configure` and `rotate`, which no longer exist.
It misses:

- `runner`, including `runner add`, which rewrote and restarted the live
  service when a test ran it on 2026-11-16;
- `update`, `engine`, `reconnect`, `login`, `auth`, `config`;
- `ai`, which calls the cloud AI assistant on the user's BYOK key.

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

---

## 5. Expected effect (PIDASHCLOU-2 as the yardstick)

| Change                                                      | Mechanism                                         | Estimated saving                            |
| ----------------------------------------------------------- | ------------------------------------------------- | ------------------------------------------- |
| §6.1–6.3 run-start materialisation, freshness, env defaults | ~7 fewer calls of ~60k each                       | ~420k tokens (~25%)                         |
| §6.6 core section + `pidash docs`                           | ~7–8k fewer tokens on each of ~19 remaining calls | ~150k tokens (~9%)                          |
| §6.8 thin skill                                             | no second manual on Codex/muse runs               | up to ~8k tokens per step on those runs     |
| §6.10 per-run block                                         | enables a cacheable static prefix                 | cost, not token count (evaluate separately) |
| **Total, first two rows**                                   |                                                   | **~1.66M → ~1.1M tokens (~−35%)**           |

Stage routing (the separate design) multiplies on top: every step it keeps is
also cheaper.

---

## 6. Changes

### 6.1 Run-start materialisation: the workpad as a file (R1)

**Location: `<working_dir>/.pidash/workpad.md`**, not the repo root. The
runner already adds `.pidash/` to `$GIT_DIR/info/exclude`
(`runner/src/cli/context.rs` `exclude_pidash_from_git`), so the file can't be
committed by accident. The current prompt's root-level scratch files
(`./.pidash-workpad.md`, `.pidash-review.md`, `.pidash-comment.md`, …) can be
committed by a careless `git add -A`, and were found left over in a test
workspace. All scratch files move under `.pidash/` (§6.6 recipes, Appendix A).

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

**Prompt** (Appendix A): "Your workpad is in `.pidash/workpad.md` (also shown
below). Edit it, then run `pidash workpad update`. If the file is missing, run
`pidash workpad get --to-file` first. If an update is rejected as changed,
run `pidash workpad get --to-file` and re-apply your edit." Update
`workpad-setup.md`, `workpad-context.md` and `test-cycle.md:41` to match.

### 6.2 Say what is fresh, and stop instructing re-reads (R1)

- Add to the inlined-context block: "The issue, comments, states and workpad
  below were read when this run started (`{{ run.rendered_at }}`). Don't
  re-fetch them. Exceptions: you have worked long enough that a human may
  have replied, you are about to make an irreversible move, or a write was
  rejected as stale." `run.rendered_at` is a new context key (UTC
  RFC 3339, set when the prompt is built), added in
  `rust-api/crates/services/src/prompting/context.rs`.
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

Prompt: core commands are shown in their short form
(`pidash comment add <id> --body-file <path> [--fold]`), and the explicit
`--as-agent` / `--agent-run-id` instructions are removed. The new prompt
ships behind a flag (§7). On a runner older than this change, a bare
`comment add` still succeeds but is attributed to the owner. That's
acceptable during rollout and is why the flag flips only after the
`MIN_RUNNER_VERSION` bump.

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

### 6.6 Core section + `pidash docs` (R5)

Rewrite `sections/pidash-cli.md` to contain only:

1. The environment/credential note (§6.4) and the output contract (one
   paragraph; the retry rule lives here only).
2. **Core commands, exact usage:** `workpad update`, `comment add`,
   `issue patch --state`, `run yield` (syntax + one line each; outcome and
   clock semantics are owned by `ending-run`, §6.7), plus `issue get`,
   `comment list`, `workpad get` as explicit refresh commands.
3. **Command index**, one line per remaining group, e.g.

   ```
   Other commands (run `pidash docs <topic>` or `pidash <cmd> --help` before first use):
     issue      create, list, search, relate/unrelate/wait, relations, run-ai, --pod/--runner
     page       project wiki pages: list, get, create, update, archive
     label      list, create, update, delete
     scheduler  view/update project schedulers
     project    runners
   ```

4. The blocklist sentence (§6.5).
5. **Conventions:** no undo, token secrecy, fenced JSON in the workpad.

Scheduler runs get the scheduler commands as their core instead of workpad /
`run yield` ticking semantics (the template already branches on `run.kind`).
Issue runs see schedulers only as one index line.

Removed from the prompt entirely:

- the `issue search` backwards-compat note (enforce a minimum runner version
  instead, §7);
- the stage-ending recipes (§6.7).

Add `pidash docs [<topic>]` to the runner CLI:

- The content is a markdown reference (`runner/docs/cli-reference.md`,
  embedded with `include_str!`) holding everything removed from the prompt:
  full flag descriptions, output fields (`relations_summary`,
  `has_open_blockers`, `dispatch_wait`, …), pods/runners policy,
  page/label/scheduler details, the remaining recipes.
- `pidash docs` lists topics; `pidash docs <topic>` prints one (`issue`,
  `comment`, `workpad`, `page`, `label`, `state`, `scheduler`, `pods`, `run`,
  `recipes`). Plain markdown on stdout, so `pidash docs | grep …` works.
- Static content only. Anything run-specific stays in the prompt.

Why not a markdown file next to the binary: same content, but adds an install
path the agent must be told and a way to drift. Why not only `--help`: help
text is the wrong place for recipes and field semantics. `--help` stays the
place for flags (and gains `--as-agent`-style details in `long_about`).

Add a rendered-size budget test per recipe kind (core section ≤ 6 KB), so it
doesn't creep back.

### 6.7 One owner per fact (R7)

| Fact                                         | Owner                              | Everyone else                               |
| -------------------------------------------- | ---------------------------------- | ------------------------------------------- |
| Command syntax and one-line semantics        | `pidash-cli` core                  | refer to it by name                         |
| `run yield` outcomes, `--stop-ticking` rules | `ending-run`                       | say _when_ to yield, not what outcomes mean |
| Which state to move to at the end of a stage | the stage's cycle section (Step 3) | –                                           |
| Workpad edit round-trip                      | `pidash-cli` core (§6.1 form)      | refer to it                                 |
| Retry rule                                   | output contract in `pidash-cli`    | –                                           |

Concretely:

- delete the stage-ending recipes from `pidash-cli.md:164-193` and the
  duplicate retry rule (`:216`);
- trim restated flags and outcome meanings in `blocking.md`,
  `task-lifecycle.md`, `analyze-and-scope.md`, `session-framing.md`,
  `workpad-setup.md`, `workpad-context.md`, `default-posture.md`,
  `guardrails.md`.

Keep one sentence of emphasis where a past failure justifies it (e.g. "never
`--stop-ticking` while an external wait remains"), but no second copy of the
specification.

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
   from fixtures exported by the Rust composer tests;
2. extracts every `pidash …` command line and parses it with the real clap
   `Command` (`try_get_matches_from`), substituting placeholders;
3. asserts every `PIDASH_*` variable the prompt promises is set by
   `apply_run_env` for that run kind;
4. asserts the never-run sentence and the generated classification (§6.5)
   agree.

Fixtures come from the Rust composer
(`rust-api/crates/services/src/prompting/`), regenerated by a test there that
fails when they are out of date. There is no Django/Rust mirror check;
Django is out of scope.

### 6.10 Per-run values in one block (R8)

- Collect per-run values at the top of the prompt in a short "This run"
  block: issue identifier, run id, stage, agent name, rendered-at time.
- Static sections refer to "this issue" / "this run" instead of templating
  ids inline. §6.3 already removes most `{{ run.id }}` uses.
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

| #   | Change                                                                                                                                                                                             | Where                  | Issue |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- | ----- |
| 1   | Runner context + workpad file: `Assign` identifiers and workpad snapshot, env exports, CLI defaults (`state list --project`, `comment add`), `body_sha256` guard, `.pidash/workpad.md` (§6.1, 6.3) | rust-api, runner       | A     |
| 2   | Agent-facing CLI: operator classification and refusal, agent-only `--help`, `pidash docs`, thin skill (§6.5, 6.6, 6.8)                                                                             | runner, skill          | B     |
| 3   | Prompt v2 behind `PROMPT_PIDASH_CLI_V2` + prompt-vs-CLI contract test + measurement (§6.2, 6.4, 6.6, 6.7, 6.9, 6.10, Appendix A)                                                                   | rust-api, runner tests | C     |

A and B can proceed in parallel. C lands after both, and the flag flips only
after the runner release that contains A and B.

**Gating step 3.** The composer can't gate on the runner's version: the
prompt is rendered at run creation
(`rust-api/crates/services/src/orchestration/creation.rs`), before the pod
matcher picks a runner. Gate it instead with:

1. **A config flag.** Add `PROMPT_PIDASH_CLI_V2` (default `False`) to the
   Rust config registry (`rust-api/crates/db/src/config/registry.rs`). With the flag off, the composer renders today's
   `pidash-cli` section with the R3/R10 corrections. With it on, it renders
   the v2 core (Appendix A).

   Implement v2 as a new section key (`pidash-cli-core`) and choose between
   the keys in `recipe_for()`, rather than one template full of
   `{% if %}`. That keeps both versions testable and lets workspace overrides
   of `pidash-cli` keep working until they opt in.

2. **A runner release** containing issues A and B, then a `MIN_RUNNER_VERSION`
   bump to it. That setting is advisory today: old runners show a red
   banner.
3. **Flip the flag** once the operator's runners are upgraded. For the
   hosted cloud, flip after the bump has been out for one release cycle.
   Self-hosters flip it themselves; the release notes say so.
4. **Fallbacks in the prompt** for an old runner that still receives v2
   (Appendix A):
   - workpad file missing → `pidash workpad get --to-file`;
   - `pidash docs` unknown → `pidash <cmd> --help`;
   - bare commands failing for lack of `PIDASH_ISSUE_IDENTIFIER` → pass
     `{{ issue.identifier }}`.

   Each costs one extra step on old runners only, never a dead end.

The `issue search` backwards-compat note is dropped from v2. Runners new
enough for v2 all have `search`.

---

## 8. Measurement

Before/after on the test runner, same issue shapes (a PIDASHCLOU-2-style
answer-only issue and one small real code issue), for `claude_code`, `codex`
and `muse_code` runners:

- first-call context tokens; rendered prompt size per kind (compiled-prompt
  preview endpoint);
- steps per run and total tokens per run, per issue;
- **redundant fetches per run**: `workpad get` / `comment list` /
  `issue get` / `state list` calls with no intervening reason. Target ~0 on
  the answer-only issue;
- failed `pidash` calls per run. Target 0 on both issues;
- `pidash docs` / `--help` lookups per run (the core is too small if this
  exceeds ~0.5 per run on typical issues);
- skill loads per run (target: no `references/` reads; `SKILL.md` loads are
  cheap);
- comments with missing `speaker_agent_run_id` from agent runs. Target 0.

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
- **Overridden sections.** Workspaces can override sections
  (user → workspace → default). Overrides of `pidash-cli` keep the old full
  text and may contradict the new runner behaviour (e.g. the workpad
  re-fetch recipe still works, it's just wasteful). Note it in the release
  notes; consider flagging overrides older than the new default in the
  customisation UI.
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
- **D3. Gating:** the composer has no runner at render time. Gate on a config
  flag (`PROMPT_PIDASH_CLI_V2`) flipped after a `MIN_RUNNER_VERSION` bump,
  with in-prompt fallbacks for old runners (§7).
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
- **D7. Workpad stays inlined in review/test prompts** as well as written to
  file. The agent must read it before deciding anything; reading the file
  instead would cost a step.

---

## 11. Implementation plan (three issues)

Tracking issue: NEWPIDASH-318. Server work is in `rust-api` only.

### Issue A: runner context and the workpad file (§6.1, §6.3)

Implements NEWPIDASH-317.

- `rust-api`:
  - At run creation (`services/src/orchestration/creation.rs`, scheduler run
    path), put `issue_identifier`, `project_identifier`, `workpad_body` and
    `workpad_sha256` into `run_config`.
  - Copy them into the `assign` envelope
    (`types/src/runner_sessions/envelopes.rs` `build_assign_msg`).
  - Workpad endpoint (`api/src/v1_work_items/handlers_relations.rs`): add
    `body_sha256` to GET/PATCH responses, and accept an optional
    `if_body_sha256` that returns 412 on mismatch.
- `runner`:
  - Add the four fields to `ServerMsg::Assign` as `#[serde(default)]`.
  - In `Supervisor::handle_assign`, write `.pidash/workpad.md` and
    `.pidash/workpad.sha256` and ensure the git exclude.
  - Extend `RunEnv` / `apply_run_env` with `PIDASH_ISSUE_IDENTIFIER`,
    `PIDASH_PROJECT` and `PIDASH_AGENT_NAME`.
  - CLI: `state list --project` plus env defaults; `comment add` speaker
    defaults; `workpad update` default path, hash precondition, keep by
    default; `workpad get --to-file`.

### Issue B: agent-facing CLI (§6.5, §6.6, §6.8)

- `runner/src/cli/mod.rs`: classify every subcommand as agent or operator,
  refuse operator commands inside runs (`PIDASH_ALLOW_OPERATOR=1` bypass),
  hide them from `--help` inside runs, fix the header.
- `pidash docs [<topic>]` from `runner/docs/cli-reference.md`
  (`include_str!`), with the content moved out of `pidash-cli.md`.
- Thin `skill/pidash/` (≤ 2 KB, no references directory), a new installer
  release, and a follow-up issue for the private overlay's plugin copies (D4).

### Issue C: prompt v2, contract test, rollout (§6.2, 6.4, 6.6, 6.7, 6.9, 6.10, §7, Appendix A)

- `rust-api/crates/services/src/prompting/`:
  - new `sections/pidash-cli-core.md` (Appendix A);
  - `PROMPT_PIDASH_CLI_V2` in `db/src/config/registry.rs`, with `recipes.rs`
    choosing between the core and full sections;
  - `run.rendered_at` in `context.rs`;
  - "This run" block and one-owner cleanup of the shared sections;
  - flag-off corrections to `pidash-cli.md` (scheduler `state list`, env
    section, "Not for you").
- Runner-side prompt-vs-CLI contract test over Rust-rendered fixtures (§6.9),
  and over `pidash docs` output.
- Rollout and measurement (§7, §8) on the test runner, with numbers posted on
  the issue.
- Follow-up investigation, after measurement: deliver static guidance through
  a cross-run cacheable channel per agent bridge (§6.10). This produces a
  short note; implementation is only filed if it's worth it.

---

## Appendix A — Draft `pidash-cli-core` section (v2)

Target ≤ 6 KB rendered. Jinja is limited to the "This run" values. Outcome
and clock semantics are owned by `ending-run`; state choice by the cycle
sections.

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

The issue, comments, available states{% if run.kind != "scheduler" %} and
your workpad{% endif %} shown in this prompt were read at
`{{ run.rendered_at }}`. Don't re-fetch them. Re-read only if you have worked
long enough that a human may have replied, before an irreversible move, or
after a write is rejected as stale.

### Core commands

{% if run.kind != "scheduler" %}

- **Workpad**: your workpad is in `.pidash/workpad.md`. Edit it, then
  `pidash workpad update`. Always write the full body; there is no append.
  If the file is missing, or an update is rejected because the workpad
  changed, run `pidash workpad get --to-file` and re-apply your edit.
  {% endif %}
- `pidash comment add <issue> --body-file .pidash/<name>.md [--fold]`: post a
  comment as this agent run. Keep scratch files under `.pidash/`. Use
  `--fold` only for low-value status notes.
- `pidash issue patch <issue> --state "<state name>"`: move an issue. Pick
  the name from "Available states".
- `pidash issue get <issue>` / `pidash comment list <issue>`: refresh only
  (see above).
  {% if run.kind != "scheduler" %}- `pidash run yield --outcome <progressed|waiting_on_human|waiting_on_external|done|blocked> [--stop-ticking] [--note "<one line>"]`:
  your **last** `pidash` call. See "Ending the run" for what each outcome
  means and when to stop the clock.
  {% else %}- `pidash issue create --project {{ project.identifier }} --title "<summary>" --description-file .pidash/finding.md`:
  file a finding.
- `pidash run yield --outcome <…> [--note "<one line>"]`: optional; if sent,
  your last `pidash` call.
  {% endif %}

### Other commands

Run `pidash docs <topic>` (or `pidash <command> --help`) before first use:

- `issue`: create, list/filter, search history, relate/unrelate/wait,
  relations, pods and dedicated runners, run-ai
- `page`: project wiki pages (read before re-deriving a convention)
- `label`: list, create, update, delete; attach via `issue patch --add-label`
- `state`: list workflow states (already shown below)
- `scheduler`: view or change scheduled automation (admin; only when asked)
- `project`: list runners

You don't need to load a `pidash` / `pi-dash` skill: this section and
`pidash docs` are the reference. Commands that manage the runner itself are
refused inside a run; if one seems necessary, the run is blocked (see
"Blocking the run"). `pidash issue re-tick` is a human decision; never call it.

### Available states

{# rendered list, unchanged from today #}

### Conventions

- All writes are real and immediate; there is no undo. Check intent against
  your workpad plan before mutating.
- When pasting `pidash` JSON into the workpad, fence it in ` ``` `.
````
