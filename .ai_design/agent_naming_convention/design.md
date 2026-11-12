# Design: agent naming convention for the Rust backend and the new frontend

- **Status:** draft for implementation, 2026-11-12
- **Scope:** Rust code (`rust-api/`, `runner/`, `pidash-ipc/`, `desktop/src-tauri/`, and the cloud overlay's `rust-cloud/`) and the new frontend (`apps/web_new/`, `packages/api-client/`)
- **Not in scope:** Django (`apps/api/`), the legacy frontend (`apps/web/`), `packages/i18n` and `packages/types` (the new frontend uses neither), database schema, and any wire or stored value
- **Code baseline:** pi-dash `main` at `e0ba74dee`; cloud overlay `main` at `0b938f8b`. Paths are relative to `rust-api/crates/` unless they start with another root.

Every "today" statement was read from the code at the baseline. Nothing was run. Production facts are from read-only queries on 2026-11-11.

## 1. Summary

Pi Dash has three kinds of AI agent and one chat product, but the code names them inconsistently, and one name points at the wrong code. This spec fixes the names in the Rust code and the new frontend, and removes the code that the wrong name is attached to.

| Step | Work | Result |
| --- | --- | --- |
| P0 | Write the vocabulary into the repo | Agents and people share one glossary |
| P1 | Stop any new issue run from selecting the cloud agent | The retired path is unreachable, not just unused |
| P2 | Gather the chat engine into one module named `cloud_agent` | `cloud_agent` means the engine the assistant runs on |
| P3 | Delete the retired issue-run code | Roughly 18,000 lines and 14 contract suites removed |
| P4 | Remove its plumbing from shared signatures, the SDK and the overlay | No dead parameters; overlay builds against the smaller SDK |
| P5 | Rename the run-creation modules that sat under the "dispatch" name | Shared infrastructure has a neutral name |
| P6 | Name the native agent "Pi Dash Agent" in Rust | One name in the backend, the daemon, the desktop app and the overlay |
| P7 | New frontend: one label, renamed identifiers, dead branches removed | Users see "Pi Dash Agent" everywhere |

P2 must land before P3: the chat engine depends on files that live inside the retired path's directories. Seven decisions in section 11 need an owner; decision 1 (a name collision with the comment bot) should be settled before P6 starts.

## 2. The convention

| Name | Meaning |
| --- | --- |
| **Pi Dash Assistant** | The chat product: the UI plus the backend for threads, messages and turns. It has two engines, chosen by where it runs. |
| **Cloud agent** | The multi-tenant, cloud-based agent engine: model routing, the model loop, provider calls, tool-call handling, usage limits. It only answers the assistant's chat in the cloud (web) version. It is not a runner and never executes an issue run. |
| **Pi Dash Agent** | The native agent built on the Codex source, installed along with the Pi Dash CLI daemon. It runs issue work, and it answers the assistant's chat in the desktop app. |
| **Third-party agents** | Agents the user brings (Claude Code, Codex, Cursor Agent, and so on), run through the daemon for issue work. |

```
 Pi Dash Assistant (chat product)
      |-- cloud version --> cloud agent      (server-side engine)
      |-- desktop app ----> Pi Dash Agent    (local, through the daemon)

 Issue runs
      |-- Pi Dash Agent
      |-- third-party agents
      (never the cloud agent)
```

Rules for code:

1. **`cloud_agent` means the chat engine and nothing else.** Code under that name that the assistant does not use is dead and is removed.
2. **The dependency reads "assistant calls cloud agent".** Assistant code (threads, turns, tools, persistence) imports the engine; the engine imports nothing from the assistant.
3. **The native agent has one name.** "engine", `managed_runner`, "built-in agent", "built-in engine", `desktop_bundled` and "bundled engine" become Pi Dash Agent in everything a developer or user reads.
4. **Stored and wire values do not change.** Anything an existing database row, an installed daemon, an older desktop build, a user's config file or Django depends on keeps its exact spelling (section 8). The Rust and TypeScript names around those values change.
5. **"engine" is not renamed blindly.** It also means "any agent process" and "the chat engine" in places (section 6.4).

## 3. Where the code contradicts the convention today

| Code today | What it actually is | Problem |
| --- | --- | --- |
| `api/src/turn/{agent,streaming,history}.rs` | The chat model loop, provider streaming and history codec | The cloud agent engine, with no "cloud agent" in its name |
| `jobs/src/oss_worker/model_runtime/{provider,usage,schemas,validation}.rs`, `UsageLimits` in `jobs/src/dispatch/execute.rs`, fixture `fixtures/dispatch/fx-disp-08-model-runtime.golden.json` | Provider lanes, usage accounting and the usage table the chat engine loads at boot | Engine code filed inside the retired issue-run path |
| `jobs/src/oss_worker/model_runtime/{agent,invoke,tools_exec}.rs`, the run/scan/sweep tasks in `jobs/src/dispatch/execute.rs`, `services/src/dispatch/{tools,tool_issues}.rs`, `prompting/sections/cloud-*.md`, `CloudAgentSettings`, tasks `cloud_agent.*`, queue `cloud_agent` | A retired path that ran the cloud agent as an issue run | Dead, and it holds the `cloud_agent` name |
| `services/src/dispatch/{policy,execution,admission}.rs`, `jobs/src/dispatch/dispatch.rs`, `db/src/dispatch/{agent_run,status}.rs` | Run creation, executor policy and dispatch for every runner | Live shared code under the retired path's name |
| `runner_engine`, `engine_credential`, `engine_state`, `ManagedRunnerSettings`, `AgentExecutorKind::ManagedRunner`, `AgentKind::Engine`, "Built-in engine", "Built-in agent", "bundled engine" | The Pi Dash Agent | Seven names for one thing |
| New frontend: "Built-in engine" (runner lists, pickers, runs), "Built-in agent" (chat contact, reason text), "Local runner" (issue panel label for `managed_runner`) | The Pi Dash Agent | Three user-visible names |

Facts that make the removal safe:

- **Nothing in the new frontend can select the cloud agent for issue work**, and every write of executor `cloud_agent` is normalized to `local_runner` (`types/src/dispatch/executor.rs:113-120`).
- **Production has no cloud agent issue runs**: 0 of 25,616 runs; all 79 projects default to `local_runner`; no issue is set to `cloud_agent`.
- **One hole remains.** `resolve_executor_kind` (`services/src/dispatch/policy.rs:294-315`) does not normalize a stored value: a project or issue row that already holds `cloud_agent` still creates a cloud run when `CLOUD_AGENT_ENABLED=true`. P1 closes it.

## 4. Target structure for the cloud agent (P2)

### 4.1 Where the engine lives

A module in the existing jobs crate: `pidash_jobs::cloud_agent` (`jobs/src/cloud_agent/`).

- **Why jobs:** the crate order is `compat → types → {auth, db} → storage → services → jobs → {api, ops} → sdk`. The engine files already depend only on jobs and services; api, the SDK and the overlay can all see jobs. No new crate edge is needed.
- **Why not a new crate:** it would have to sit below jobs and take ownership of several jobs types to avoid a cycle. That is possible later; it is not needed to make the names true.

### 4.2 File moves

| From | To | Notes |
| --- | --- | --- |
| `api/src/turn/agent.rs` (1,616 lines) | `jobs/src/cloud_agent/agent.rs` | The model loop (`run_agent`), the `Toolset` trait. No api-crate imports today |
| `api/src/turn/streaming.rs` (4,062) | `jobs/src/cloud_agent/streaming.rs` | Provider SSE streaming for both lanes |
| `api/src/turn/history.rs` (1,869) | `jobs/src/cloud_agent/history.rs` | Codec for stored model messages |
| `api/src/turn/replay.rs` (313, test-only) | `jobs/src/cloud_agent/replay.rs` | Test harness; needs `axum` as a jobs dev-dependency |
| `jobs/src/oss_worker/model_runtime/provider.rs` (2,480) | `jobs/src/cloud_agent/provider.rs` | Keep what chat uses: `Lane`, `lane_for`, `WireTool`, the two schema transforms, `Finish`, `ToolArgs`, `ProviderError`, `py_repr`, `generate_tool_call_id`, `INVALID_JSON_KEY`, `USER_AGENT`, `ANTHROPIC_VERSION`. The non-streaming lane (`complete`, `parse_*_response`, `Turn`, `RequestPart`, `ResponsePart`, `RetryContent`, `HistoryMessage`, the request-body builders) is deleted in P3 |
| `jobs/src/oss_worker/model_runtime/usage.rs` (930) | `jobs/src/cloud_agent/usage.rs` | `UsageLimits` moves in from `jobs/src/dispatch/execute.rs:583` |
| `jobs/src/oss_worker/model_runtime/schemas.rs` (199) | `jobs/src/cloud_agent/usage_table.rs` | Becomes a loader for the usage table only (section 4.4) |
| `jobs/src/oss_worker/model_runtime/validation.rs` (909) | `jobs/src/cloud_agent/validation.rs` | Keep `unknown_tool_content`; keep `ValidationDetail`, `LocSeg`, `ensure_ascii` only if `provider.rs` still needs them after P3 |
| `jobs/src/oss_worker/model_runtime/keys.rs` (398) | `jobs/src/cloud_agent/keys.rs` | `KeyResolver` is live: the overlay's chat routing uses it |
| `jobs/src/oss_worker/model_runtime/github.rs` (1,394) | `jobs/src/integrations/github_app.rs` | Not engine code. Live: the overlay's MCP pull-request tools use it |
| `jobs/src/assistant/provider_errors.rs` (275) | `jobs/src/cloud_agent/errors.rs` | Provider error classification |
| `AgentError`, `StreamItem` in `jobs/src/assistant/run_turn.rs` | `jobs/src/cloud_agent/mod.rs` | The engine's boundary types. `run_turn` re-exports them so existing paths keep compiling |

After P2 and P3 the directory `jobs/src/oss_worker/model_runtime/` no longer exists.

Left where it is, on purpose:

- `services/src/assistant/{llm,retry}.rs` (`ModelRef`, retry policy): engine-level, but in a lower crate that jobs already imports. Moving them is optional and not part of this work.
- `REQUEST_LIMIT` and `TOOL_CALLS_LIMIT` (`run_turn.rs`): these are the assistant's limits. The assistant builds a `UsageLimits` and passes it to the engine, as `turn_usage_limits()` does today; that function stays on the assistant side.

### 4.3 What stays assistant

| Path | Why |
| --- | --- |
| `api/src/turn/live.rs` (`LiveTurnSeam`) | Thread and turn rows, events, history load, cancel, BYOK decrypt, the MCP and local toolset bridge. It becomes the visible call site: `open_stream` calls `cloud_agent::agent::run_agent` |
| `api/src/turn/{tools_local,tools_local_store,tool_schemas,tool_args,mcp_client,instructions}.rs` | The 18 Pi Dash tools, their store, the MCP client and the system prompt. Tied to the api crate |
| `jobs/src/assistant/{run_turn,sweep}.rs` | The turn driver (`TurnSeam`, `drive_turn`) and the stale-turn sweep |
| `services/src/assistant/*` | Config, crypto, markdown, MCP planning, the pure halves of the tools |

`live.rs` keeps a small amount of engine set-up (choosing the lane, converting tool schemas to `WireTool`, loading the usage table). Move those into two engine helpers, `cloud_agent::provider::wire_tools_for(lane, schemas)` and `cloud_agent::usage_table::load()`, so `live.rs` calls the engine and holds no engine logic.

### 4.4 The boot fixture

The chat engine loads `fixtures/dispatch/fx-disp-08-model-runtime.golden.json` at boot (`live.rs:870` calls `Golden::load()`), and `Golden::load()` fails unless the file holds exactly 15 retired tool schemas and an output schema. Chat reads only the `usage_providers` section.

1. Split the fixture: `fixtures/cloud_agent/usage-providers.golden.json` holds `usage_providers` (used at run time) and `usage_vectors` (used by the usage tests). The remaining sections go with the retired tests in P3.
2. Change the loader in the same pull request: `usage_table::load()` reads the new file and requires only the two sections. `ProviderTable::load` no longer takes a `Golden`.
3. The `include_str!` path is relative to the source file, so it changes with the move. The test fixtures under `fixtures/assistant/turn-runtime/` are found through `CARGO_MANIFEST_DIR/../../fixtures`, which still resolves from `crates/jobs`.

Doing step 1 without step 2 stops chat from booting.

### 4.5 SDK paths stay the same

The overlay compiles only against `pidash_sdk`. Every name it uses keeps its SDK path; only the `pub use` source in `sdk/src/lib.rs` changes.

| SDK name (unchanged) | New source |
| --- | --- |
| `pidash_sdk::assistant::{Lane, ProviderError}` | `pidash_jobs::cloud_agent::provider` |
| `pidash_sdk::assistant::KeyResolver` | `pidash_jobs::cloud_agent::keys` |
| `pidash_sdk::assistant::{GithubAppContext, GithubRestClient, GithubFailure, github_headers, installation_token, GithubHeadersError, CLIENT_USER_AGENT}` | `pidash_jobs::integrations::github_app` |
| `pidash_sdk::assistant::classify_provider_error`, `BYOK` | `pidash_jobs::cloud_agent::errors` |
| `pidash_sdk::assistant::{AgentError, StreamItem}` | `pidash_jobs::cloud_agent` |
| The 17 `turn::history` names (`TurnMessage`, `parse_blob`, …) | `pidash_jobs::cloud_agent::history` |

Add a new SDK module `pidash_sdk::cloud_agent` that re-exports the engine's public surface, and keep the `pidash_sdk::assistant::*` paths above as they are. `SURFACE.txt` and `tests/surface.rs` gain the new module; no existing line changes. P2 therefore needs no overlay change.

## 5. Removing the retired issue-run path (P1, P3, P4)

### 5.1 P1: make it unreachable

| # | Change | File |
| --- | --- | --- |
| 1.1 | In `resolve_executor_kind`, map a resolved `CloudAgent` to `LocalRunner`, as `ManagedRunner` already is at line 309. Drop `ResolveExecutorError::CloudAgent` | `services/src/dispatch/policy.rs:294-315` |
| 1.2 | Treat a `cloud_agent` write on the non-pod paths as normalized instead of gated on `CLOUD_AGENT_ENABLED` | `api/src/app_issues/handlers_core.rs:2695-2697`, `api/src/app_workspace/handlers_favorites_drafts.rs:6638-6639` |
| 1.3 | Remove the project-create bypass that accepts `DEFAULT_AGENT_EXECUTOR=cloud_agent` | `api/src/app_project/handlers_project.rs:4647-4656` |
| 1.4 | Data check on each environment before deploy (read-only): non-terminal `agent_run` rows with `executor_kind='cloud_agent'`; `projects.default_agent_executor='cloud_agent'`; `issues.agent_executor='cloud_agent'`; `rust_job_queue` rows with `queue='cloud_agent'` or `task LIKE 'cloud_agent.%'` | — |

Contract tests that assert cloud creation change with this step: `contract-tests/runner/test_web_runs.py:27,459-477,835-860`.

### 5.2 P3: delete, keeping the workspace compiling at each step

**Step A, the execute plane.**

| Action | Target |
| --- | --- |
| Move out first | The managed expire task (`managed_runner.expire_waiting_runs` and its helpers in `jobs/src/dispatch/execute.rs:133-2620`, plus `finalize_agent_run` in `jobs/src/oss_worker/execute_seams.rs:214-450`) to `jobs/src/runner_runs/waiting_expiry.rs`. It is live: it serves Pi Dash Agent runs that never came online |
| Delete whole files | `jobs/src/oss_worker/model_runtime/{agent.rs (2,671), invoke.rs (355), tools_exec.rs (4,670)}`; `bin/pidash-api/src/{execute_seams.rs, model_runtime.rs}` and their `mod` lines in `main.rs:20,23`; `types/src/dispatch/{output.rs (384), error.rs (509)}`; `db/src/dispatch/{event.rs (149), tool_call.rs (356)}` |
| Delete the rest of | `jobs/src/dispatch/execute.rs` (5,693 lines; everything except the expire path and `UsageLimits`, both already moved); the run-model half of `execute_seams.rs` |
| Delete from `jobs/src/dispatch/dispatch.rs` | `RUN_CLOUD_AGENT_TASK`, `run_cloud_agent_*`, `validated_job_queue`, `dispatch_capacity`, `lease_expiry_at`, the event-cap helpers, the running/offered/due queries, `append_event`, `DispatchDecision::CloudDispatched` |
| Delete registrations | The three cloud task registrations and the two beat entries (`jobs/src/schedule.rs:357-368`, `jobs/src/oss_worker/mod.rs:428-469`) |
| Update count assertions | Schedule length 31 → 29 (`schedule.rs:569`, `scheduler.rs:362,403`, `bin/pidash-api/src/inventory.rs:2204`); worker-owned task count 80 → 77 (`inventory.rs:2177`) and 79 → 76 (`oss_worker/mod.rs:634`); `OWNED_BEAT_ENTRIES` 3 → 1; the scheduler allowlist test (`scheduler.rs:365-389`) uses the two cloud entry names and needs other names |

`dispatch_waiting` becomes a function that returns `Ok(0)` in this step, and is removed in P4 with its trait methods.

**Step B, policy and creation.**

| Action | Target |
| --- | --- |
| Delete | `services/src/dispatch/tool_issues.rs` (309); in `policy.rs`: the tool-name lists, `build_tool_plan`, `resolve_current_tool_names`, `CloudCapabilityUnavailable`, `RequiredToolUnavailable`, the GitHub-binding checks; in `execution.rs`: `execution_fields_cloud`, `lock_cloud_creation_capacity`, `quota_verdict`, the queued-cloud counts; in `admission.rs`: `CloudAgentAdmissionError`, `enforce_creation_rate`, `cloud_agent_configuration_check`, `CloudAgentUnavailableBody` |
| Move, then delete the file | From `services/src/dispatch/tools.rs` (3,056) move the live helpers `advisory_lock_key`, `saved_sequence_id`, `create_save_state_select`, `create_seq_scan_select` and the `CREATE_*`/`ISSUE_SORT_ORDER_*` constants to `services/src/domain/issue_create_sql.rs`; keep the SDK `ops::*` re-exports pointing at the new home; delete the rest |
| Delete | The `CreationSeam::lock_cloud_creation_capacity` method and its five implementations; `execution_fields` collapses to `resolve_executor_kind` plus an empty tool plan |
| Delete | Prompting: the 16 `prompting/sections/cloud-*.md` files, `CLOUD_RECIPES`, `cloud_recipe_for`, `compose_cloud`, the cloud branches in `prompting/context.rs` (`:1164`, `:1277`, `:1325`, `:1417`), `direct_context`; the cloud direct-run code in `api/src/runner_runs/dispatch.rs:1089-1360,1637-1795` |
| Delete tests and fixtures | 14 suites under `contract-tests/dispatch/` (keep `test_expire_waiting_runs.py`; trim `test_beat_schedule.py`, `test_celery_wire.py`, `conftest.py`); fixtures `fx-disp-05`, `06`, `07`; the retired sections of `fx-disp-08`; workflow `rust-api-contract-tests-dispatch-rust.yml` (move `assistant/test_turn_isolation.py` to another job first) |

API effect: the `cloud-*` prompt section keys answer 404 instead of 200 on the endpoints in `api/src/prompting/mod.rs`.

**Step C, branches on the executor.** Only historical cloud rows can exist, and they are terminal.

| Location | Becomes |
| --- | --- |
| Cancel handler `api/src/runner_runs/runs.rs:1837-1851,1934-1953,2031-2041` | Delete the cloud arms; drop `!= CloudAgent` from the condition at `:1954` |
| Cloud-only terminal-event appends in the generic finalizers (`run_endpoints.rs:651`, `runner_open.rs:837`, `runner_poll.rs:438`, `offline_recovery.rs:283`, `sweeps_runs.rs:1007`, `runner_enroll.rs:601`, `machine_merge.rs:341`, `creation_jobs.rs:2118`, `handoff.rs:2140`) and their helpers | Delete |
| `CapacityPlan::Cloud` (`services/src/runner_runs/finalization.rs:737-745`) | A no-op that still stamps `terminal_capacity_released_at`. Without the stamp, the reconcile beat retries a historical row forever |
| `LifecycleEffect::DispatchWaiting`, the cloud branch of `dispatch_agent_run` (`dispatch.rs:504-510`) | Delete |
| Runs list and detail serialization of `tool_plan` and `tool_calls` (`services/src/runner_runs/shape.rs`, `runs.rs:493,1086-1160`) | **Keep.** Historical rows must still display |
| `AgentExecutorKind::CloudAgent`, the choice lists, write normalization | **Keep** (section 5.4) |

### 5.3 P4: plumbing, SDK and overlay, in lockstep

These remove public names the overlay compiles against, so the open-source pull request and the overlay's pin-bump pull request merge together.

| Remove in open source | Overlay change in the same pin bump |
| --- | --- |
| `CloudAgentToolsetsSeam`, `NoExtraToolsets`, `ExtraToolset`, `ToolsetsRun`, `ToolsetsResolution`, `ToolsetsUnavailable`, `AppState::with_toolsets`, `jobs::extra_toolsets_allowed` | Delete `cloud-api/src/openhub/toolsets.rs`, its registration in `bin/pidash-cloud/src/main.rs:163,182-193`, the `toolsets` parameters in `mcp/views.rs` and `mcp/tools_issues_write.rs`, and the toolsets contract tests |
| `AssistantModelSeam::resolve_model_for_user`, `ExecuteModelInputs`, `ResolvedRunModel`, `RunModelKey`, `CloudAgentModelSeam` | Delete the run-model implementation on `CloudAssistantSeam` (`cloud-api/src/assistant/provider.rs:1410-1500`), `gateway_credential_for_run`, `byok_run_model`. Keep the free function `resolve_model_for_user` (chat uses it) and `gateway_to_assistant_error` |
| `AdmissionCache`, `RedisAdmissionCache`, `DeferredConsume`, `consume_admission_token`, `MoveAdmissionCache`, the `HookDeps.{cloud,cache,toolsets}` fields | Drop the fields where `HookDeps` is built (`mcp/tools_issues_write.rs:544-551`) |
| `RunnerPorts::dispatch_waiting`, `RunnerRunsSeams::dispatch_waiting`, `ops::dispatch_waiting` | Drop the fake implementations in `platform/{access,consume,events,reconcile,workspaces}.rs` |
| `CloudAgentSettings`, `Settings.cloud_agent`, the 33 `CLOUD_AGENT_*` registry keys, and the `cloud` parameter threaded through about 20 files | Drop the `settings.cloud_agent` reads in `platform/*.rs` and `mcp/tools_issues_write.rs:546` |

Deploy clean-up, after the code is live:

- Overlay `docker-compose.rust.ecr.yaml`: remove the `agent-worker` service; drop the two cloud entries from `PIDASH_BEAT_ONLY`, keeping `agent-run-reconcile-terminal-effects`.
- Overlay `.github/workflows/release.yml:480,493,539,541`: remove `agent-worker` from the profiles and the health loops.
- Remove the `CLOUD_AGENT_*` parameters from SSM.

The generic worker options added with the dedicated queue (`--queue`, `--no-scheduler`, `PIDASH_BEAT_ONLY`, `DATABASE_MAX_CONNECTIONS`, the shutdown grace) stay. They are not cloud-specific, and section 12 lists what should use them next.

### 5.4 What stays for historical rows

- The stored executor value `cloud_agent` and the enum variant that parses it. Rename the Rust variant to `AgentExecutorKind::RetiredCloudRun` with the string value unchanged, so the name `CloudAgent` no longer appears on run code.
- The `agent_run_tool_call` table and its reads; `agent_run.tool_plan`, `lease_expires_at`, `dispatch_attempts`; the `agent_run_cloud_*` constraints and indexes.
- The new frontend's read-only rendering of old cloud runs (section 7.3).

## 6. Pi Dash Agent in Rust (P6)

### 6.1 Code spelling

`pidash_agent` for modules and functions, `PidashAgent` for types, "Pi Dash Agent" for anything shown to a user. This matches the existing `pidash` crate and binary spelling, and it differs from the comment bot's stored username `pi_dash_agent` (decision 1).

### 6.2 Backend (`rust-api`)

| Today | Becomes |
| --- | --- |
| Module `api/src/runner_engine/` | `api/src/pidash_agent/` |
| `services/src/domain/engine_credential.rs`, `engine_state.rs` | `domain/pidash_agent_credential.rs`, `domain/pidash_agent_state.rs` |
| `services/src/runner_enroll/engine_naming.rs`, `jobs/src/runner_runs/engine_pause.rs` | `pidash_agent_naming.rs`, `pidash_agent_pause.rs` |
| `types/src/dispatch/managed_runner.rs`: `ManagedRunnerReason`, `ManagedRunnerUnavailable` | `pidash_agent.rs`: `PidashAgentReason`, `PidashAgentUnavailable` (code strings unchanged) |
| `AgentExecutorKind::ManagedRunner` | `AgentExecutorKind::PidashAgent`, string value still `managed_runner` |
| `is_engine_runner`, `is_engine`, `wants_engine`, `engine_named` | `is_pidash_agent_runner`, `is_pidash_agent`, `wants_pidash_agent`, `pidash_agent_named` |
| `managed_runner_availability`, `managed_runner_probe`, `ManagedRunnerProbe` | `pidash_agent_availability`, `pidash_agent_probe`, `PidashAgentProbe` |
| `issue_engine_credential`, `authenticate_engine_credential`, `engine_profile_for_user`, `ENGINE_CREDENTIAL_PREFIX`, `PROVISIONING_DESKTOP_BUNDLED`, the `ENGINE_*` constant names | The same with `pidash_agent` / `PIDASH_AGENT`; constant values unchanged |
| Comments, doc comments and `tracing` messages that say "engine", "built-in engine", "managed runner" for this agent | "Pi Dash Agent" |

SDK-coupled names move with one overlay pin bump, or keep a deprecated alias for one cycle: `ManagedRunnerSettings` and `Settings.managed_runner`, `AgentModelProfile`, `agent_model_profile`, `agent_model_credential`, `agent_model_credential_for_user`, `DesktopRequestGate`, `DesktopCredential`.

Entity modules under `db/src/entity/` (`engine_credential`, `dev_machine`) keep their names: they follow the table names.

### 6.3 Daemon, IPC and desktop

| Today | Becomes | Kept exactly |
| --- | --- | --- |
| `runner/src/engine/` (14 files), `cli/engine_cmd.rs`, `daemon/engine_ops.rs`, `daemon/engine_reconcile.rs`, `ipc/engine_chat.rs` | `runner/src/pidash_agent/`, `cli/pidash_agent_cmd.rs`, `daemon/pidash_agent_ops.rs`, `daemon/pidash_agent_reconcile.rs`, `ipc/pidash_agent_chat.rs` | — |
| `AgentKind::Engine` (`runner/src/config/schema.rs:584-611`) | `AgentKind::PidashAgent` | `#[serde(rename = "engine")]` and `#[value(name = "engine")]`. Also pin the `agent_versions` key, which is derived from the variant's `Debug` name today (`daemon/supervisor.rs:1634`) and would otherwise change on the wire |
| `display_name` "Built-in engine"; CLI help, TUI label, `install.sh` and `install.ps1` messages | "Pi Dash Agent" | — |
| Types `EngineSection`, `EngineCredential`, `EngineProfile`; IPC types `EngineState`, `EngineStatusInfo`, `EngineSignInState` | `PidashAgent*` | Every serde tag, IPC method name and JSON field |
| Desktop Rust identifiers (`engine_home`, `legacy_engine_home`, helpers in `daemon.rs`, `setup.rs`, `migration.rs`) | `pidash_agent_*` | Tauri command names; on-disk paths |

### 6.4 "engine" that is not the Pi Dash Agent: do not rename

| Use | Where |
| --- | --- |
| Any agent process | `runner/src/agent/mod.rs:33-56` (`ENGINE_STARTUP_TIMEOUT`), `daemon/runner_instance.rs:53` (`engine_version` is the probed agent's version) |
| The hello key `engine_version`, stored as `codex_version` | `session_service.rs:231` |
| The assistant's chat engine | Overlay `cloud-api/src/assistant/provider.rs:1546+` |
| Unrelated | `list_engine`, the recurrence engine, `base64::Engine`, prompt rendering |

### 6.5 Overlay

Rename comments and doc text that call the native agent "engine" or "desktop engine" in `cloud-api/src/assistant/provider.rs` (`:11`, `:128`, `:1206-1207`, `:1269`, `:1319`). Rename the SDK-tied identifiers in the same pin bump as section 6.2. The frozen migration ports under `cloud-migration/sql/` and `sources.json` never change.

## 7. New frontend (P7)

### 7.1 One label

Add `PIDASH_AGENT_NAME = "Pi Dash Agent"` in `apps/web_new/src/shared/lib/runnerIdentity.ts` and use it for every user-visible mention. Remove the inline duplicate in `shared/pickers/pods.ts:171`.

| Today | Where | Becomes |
| --- | --- | --- |
| "Built-in engine", "Built-in engine · <machine>", badge "Built-in" | `shared/lib/runnerIdentity.ts:31,34,63-66`; `shared/pickers/pods.ts:171`; `PodPicker.tsx` | "Pi Dash Agent", "Pi Dash Agent · <machine>", badge "Pi Dash Agent" |
| "Built-in agent", "The built-in agent is unavailable." | `features/runners/chat/localContact.ts:33,73`; `localTransport.ts:241,330,666` | "Pi Dash Agent", "Pi Dash Agent is unavailable." |
| Section header "Built-in" | `features/runners/components/RunnersRail.tsx:22` | "Pi Dash Agent" |
| Reason sentences ("The built-in agent is switched off…", "The desktop agent runs on…") | `shared/lib/agentReasons.ts:23,33,37-42` | Same sentences with "Pi Dash Agent" |
| Runtime text ("…in Built-in engine settings…", "The built-in engine needs a new sign-in.") | `features/runners/runtime/service.ts:61-64,194,457`; `DaemonSetupDialog.tsx:23,34`; `ReplaceMachineDialog.tsx:33`; `SignOutDialog.tsx:56`; `MigrationNoticesDialog.tsx` | "Pi Dash Agent" |
| Machine table and dialogs ("Pause the built-in engine", "Sign out the engine", "Engine paused by you", "Engine not signed in", …) | `features/runners/devMachines/{DevMachinesTable.tsx:51-62,80, model.ts:107,152-182, DevMachineSettingsDialogs.tsx:16,96-105, DevMachinesScreen.tsx:38-42}` | "Pi Dash Agent" |
| Remove and rename hints | `components/RunnerConfirmDialog.tsx:36,40`; `RunnersOverview.tsx:38`; `detail/RunnerDetailScreen.tsx:32`; `podMembers.ts:71` | "Pi Dash Agent" |
| Run executor label for `managed_runner` | `features/runners/runs/runsModel.ts:21-28` | "Pi Dash Agent" |
| Issue panel labels `managed_runner` as "Local runner" | `features/issues/detail/agent/agentStatus.ts:407-410` | "Pi Dash Agent" |
| "the engine's sign-in expired", "the engine is disabled" | `features/issues/detail/waiting/waiting.ts:56,59` | "Pi Dash Agent's sign-in expired", "Pi Dash Agent is disabled" |
| "The agent engine is not part of this app…" | `features/about/DesktopAboutControl.tsx:31-32` | "Pi Dash Agent is not part of this app…" |

"This computer" names the machine, not the agent, and stays.

End-to-end expectations of the old text change with it: `e2e/parity/desktop/agent-runtime/agent-runtime.spec.ts` (32 places), `e2e/parity/runners/dev-machines-detail.spec.ts`, `e2e/parity/issues/pod-dropdown.spec.ts`, `e2e/parity/drivers/web.ts`. One negative assertion must be rewritten: `e2e/parity/issues/pod-runner-picker.spec.ts:469` requires the text "pi dash agent" to be absent.

### 7.2 Identifiers

| Today | Becomes |
| --- | --- |
| `isEngineRunner`, `ENGINE_PROVISIONING`, `BUILT_IN_ENGINE`, `BUILT_IN_BADGE`, `engineLabel`, `ENGINE_AUTO_PAUSE_TEXT` | `isPidashAgentRunner`, `PIDASH_AGENT_PROVISIONING`, `PIDASH_AGENT_NAME`, `PIDASH_AGENT_BADGE`, `pidashAgentLabel`, `PIDASH_AGENT_AUTO_PAUSE_TEXT` |
| `MANAGED_RUNNER_KIND`, `isManagedRunnerAvailable` | `PIDASH_AGENT_EXECUTOR_KIND`, `isPidashAgentAvailable` |
| `EngineUnavailableError`, `renewEngineIfAllowed`, `assertEngineUsable`, `approveEngineGrant`, `turnOnEngineForProject`, `hasEngine`, `engineActionFor`, `EngineAction`, `EngineStateView`, `MachineEngineContext`, `localEngineRunner` | The same with `PidashAgent` |
| `signOutEngine`, `removeEngine`, `useSignOutDevMachineEngine`, the `*_ENGINE_*` copy constants, dialog kinds `"sign-out-engine"` and `"remove-engine"` | The same with `PidashAgent` / `pidash-agent` |
| `LocalContact`, `LOCAL_CONTACT_NAME`, `BUILTIN_SECTION_HEADER`, `builtinRunnerRow` | `PidashAgentContact`, `PIDASH_AGENT_NAME`, `PIDASH_AGENT_SECTION_HEADER`, `pidashAgentRunnerRow` |
| Platform types `DaemonEngineState`, `DaemonEngineStatus`, `DaemonEngineRunner`, `addEngineRunner`, `ensureEngine`, `engineLogin` (`core/platform/types.ts:89-223`) | `DaemonPidashAgent*`, `addPidashAgentRunner`, `ensurePidashAgent`, `pidashAgentLogin`. The Tauri command strings they invoke do not change |
| api-client exports `getAgentProfile`, `postAgentToken`, `signOutDevMachineEngine`, `ENGINE_AUTO_PAUSE_REASONS`, `ENGINE_SIGN_IN_STATES`, `EngineAutoPauseReason`, `EngineSignInState` | `getPidashAgentProfile`, `postPidashAgentToken`, `signOutDevMachinePidashAgent`, `PIDASH_AGENT_AUTO_PAUSE_REASONS`, `PIDASH_AGENT_SIGN_IN_STATES`, and the matching types. Paths and field names unchanged |
| End-to-end helpers `cloudAgentProfileRaw`, `cloudAgentTokenRaw` (`e2e/parity/helpers/api.ts:21211,21219`) and spec `e2e/parity/cloud/desktop-agent.spec.ts` | `pidashAgentProfileRaw`, `pidashAgentTokenRaw`, `e2e/parity/desktop/pidash-agent-profile.spec.ts`. They call the Pi Dash Agent's profile and token endpoints; "cloud" in the name was wrong |

### 7.3 Cloud agent leftovers in the new frontend

| Location | Action |
| --- | --- |
| `features/runners/runs/RunDetail.tsx:195-235` (tool plan and tool calls panel), `runsModel.ts` (`executorLabel`, `runExecutorLabel`, `executorTone`, `resultSummary`), `agentStatus.ts:407-410` | Keep: old cloud runs must still display. Change the label from "Cloud agent" to "Cloud agent run (retired)" |
| `RunDetail.tsx:33-34,46-48` (`RUN_CANCEL_CLOUD_BODY`, `cancelBodyKind`) | Delete: a historical cloud run is terminal, so cancel is never offered |
| `agentStatus.ts:218-219,237-238` ("Waiting for cloud agent capacity.", "The cloud agent is working with the project's tools.") | Delete: a queued or running cloud run cannot exist |
| `shared/lib/agentReasons.ts:45` (`cloud_agent_unavailable`) | Delete if the deprecated option row is dropped (decision 4) |
| `packages/api-client/src/contracts/runs.ts:31` (`EXECUTOR_KINDS` with `cloud_agent`), `RunToolCall`, `RunToolPlan`, `projects.ts:61` (`agent_executor_options`) | Keep: contract fields the API still sends |

## 8. Values that must not change

| Value | Why | Other party |
| --- | --- | --- |
| URLs `/api/v1/runner/engine/{profile,model-token,credential/revoke}/`, `/api/users/me/ai-assistant/{agent-profile,agent-token}/`, `/api/runners/dev-machines/{id}/engine-sign-out/`, `/api/v1/runner/dev-machines/desktop-enroll/`, query `?include_bundled` | Called by installed daemons, the desktop app and Django routes | `runner/src/engine/api.rs:34-36`; `packages/api-client`; Django |
| Stored executor values `managed_runner`, `cloud_agent`; `provisioning` values `desktop_bundled`, `manual` | Database rows | Django migrations; overlay frozen ports |
| Table `engine_credential`; `dev_machine.engine_*` columns; `engine_auto_pause_reason` values; `cli_device_codes.scope = 'engine'`; runner names `builtin-…`; capability `agent:engine`; credential prefix `ec_` | Database | Django owns the schema |
| JSON fields `engine_*`, `is_engine`, `engine_paused`, `engine_state`, `engine_credential`, `engine_sign_in_state`, `managed_runner_enabled`, `agent_kind: "engine"`, `scope: "engine"` | Wire | Daemon, api-client, Django |
| Reason and error codes (`managed_runner_disabled`, `no_managed_runner_for_project`, `managed_runner_paused`, `engine_sign_in_required`, `engine_disabled`, `engine_runner_project_fixed`, `desktop_session_required`, …) | Wire | New frontend, daemon IPC, Django |
| Task names `managed_runner.expire_waiting_runs`, `runner.pause_engines_with_expired_credentials` and their beat keys | Shared with Django's task set | Django `celery.py` |
| Environment keys `MANAGED_RUNNER_*`, `DESKTOP_MIN_VERSION_FOR_MANAGED_RUNNER` | Read by Django too | Operators, test harnesses |
| Daemon config: `[runner.agent] kind = "engine"`, `[engine] binary`, `[engine] task_root`, `[cli] engine_credential`, `[cli] engine_sign_in_state`; keyring item `engine-credential` | Files on users' machines | Existing installs |
| Paths `<data>/engine/…`, install directory `engine/<version>/`, binary `pidash-agent-engine`, files `ENGINE_VERSION`, `ENGINE_SHA256` | On disk and in release artifacts | Installers, `dist-workspace.toml`, WiX |
| CLI verbs `pidash engine` (`install`, `path`, `pause`, `resume`, `status`), `pidash login --engine`, `pidash auth logout --engine`, `pidash runner add --agent engine`, hidden `pidash __engine model-token` | Invoked by the desktop app and install scripts; the hidden one is written into existing Codex config files | `desktop/src-tauri/src/setup.rs:263-357`; `runner/install.sh:89`; `engine/codex_config.rs:76-77` |
| IPC method names `engine_*`, `HelloInfo.engine`, `engine_detail` (IPC version 6) | Desktop and daemon may be different versions | `pidash-ipc/src/protocol.rs:100-129,442-446` |
| Tauri command names `daemon_engine_runner_add`, `daemon_ensure_engine`, `pidash_cli_engine_login`, `pidash_cli_logout` | Desktop build and the frontend bundle must agree | `core/platform/tauri.ts:569-594` |
| Desktop chat store columns `engine_version`, `engine_thread_id`; chat id `pidash-builtin`; data directory `managed/` | Local SQLite and paths | `desktop/src-tauri/src/chat_history.rs` |
| Bot user `users.username = 'pi_dash_agent'` | Database | Decision 1 |
| Server-written English sentences that contract tests pin byte-for-byte to Django ("The engine is turned off.", "Sign the built-in engine in again…", choice labels) | Django parity; Django is not changing | Decision 3 |

## 9. Order, size and verification

| Phase | Repos | Depends on | Check before merging |
| --- | --- | --- | --- |
| P0 | pi-dash | — | Glossary section added to `CLAUDE.md`, pointing at this doc |
| P1 | pi-dash | — | Runner and work-item contract suites; the data check in 5.1 returns zero rows on test and production |
| P2 | pi-dash | — | `cargo test -p pidash-jobs cloud_agent::`, `cargo test -p pidash-api turn::`; contract suites `assistant/` and `tasks_assistant/`; `SURFACE.txt` gains lines and loses none; overlay builds unchanged against the new commit |
| P3 | pi-dash | P1, P2 | Whole-workspace `cargo clippy --all-targets -- -D warnings` and `cargo test`; `assistant/`, `tasks_assistant/`, `runner/`, `v1_work_items/`, `orchestration/` suites; `test_expire_waiting_runs.py` still passes |
| P4 | pi-dash + overlay, merged together | P3 | Overlay `check`, `assistant`, `mcp`, `platform`, `boot`, `migrations` jobs; a chat message on the test stack gets a reply |
| P5 | pi-dash (+ overlay if SDK names move) | P4 | Compile and the suites of P3 |
| P6 | pi-dash, overlay | P4 | `cargo test` in `runner/`; daemon round-trip tests for the config file and IPC (an old config with `kind = "engine"` still loads; the `agent_versions` key is unchanged); desktop build |
| P7 | pi-dash | P6 for API-client names only | Frontend type check, unit tests, and the parity specs named in 7.1 |

P5, the neutral names for shared run code:

| Today | Becomes |
| --- | --- |
| `services/src/dispatch/` (what remains: executor policy, `execution_fields`, Pi Dash Agent availability) | `services/src/run_creation/` with `executor_policy.rs`, `execution.rs`, `pidash_agent_availability.rs` |
| `jobs/src/dispatch/dispatch.rs` (what remains: `dispatch_agent_run`, `DispatchDecision`) | `jobs/src/run_dispatch.rs` |
| `db/src/dispatch/{agent_run,status}.rs` | `db/src/agent_runs/` |
| `types/src/dispatch/{executor,managed_runner}.rs` | `types/src/agent_runs/{executor,pidash_agent}.rs` |

Each phase is one or more pull requests of pure moves or pure deletions. A move and a behaviour change never share a pull request, so a reviewer can check a move by confirming nothing else changed.

## 10. Risks

| Risk | Mitigation |
| --- | --- |
| Chat stops booting because the fixture was trimmed without the loader change | Section 4.4: one pull request for both; a boot test constructs `LiveTurnSeam` |
| The overlay breaks on an SDK change | P2 changes no SDK path. P4 and the SDK-coupled part of P6 ship with the overlay pin bump in the same merge window |
| A historical cloud run is retried forever by the reconcile beat | `CapacityPlan::Cloud` still stamps `terminal_capacity_released_at` (5.2 step C) |
| Leftover `cloud_agent.*` jobs or `queue='cloud_agent'` rows park as "unknown task" after the handlers are gone | The data check in 5.1; drain or delete those rows before P3 deploys |
| Renaming `AgentKind::Engine` changes a wire key derived from its `Debug` name | Pin the key explicitly in the same change (6.3); add a test on the serialized attach body |
| A blind "engine" rename hits generic uses | The exclusion list in 6.4; rename by listed identifier, not by search and replace |
| The parity job inventory reports the removed tasks as drift, because Django still lists them | Add an expected-gap entry in `contract-tests/parity/EXPECTED.md` |
| Open pull requests for NEWPIDASH-190 modify files P3 deletes (`tools_exec.rs`, `services/src/dispatch/tools.rs`) | Close or re-target those pull requests before P3. The shared `operations::core` modules they add are not touched by this spec |
| Prompt overrides stored for `cloud-*` section keys become unreachable | Check the override table for those keys before P3; none are expected |

## 11. Open decisions

| # | Decision | Recommendation |
| --- | --- | --- |
| 1 | "Pi Dash Agent" is already the display name of the comment bot user (username `pi_dash_agent`, first name "Pi Dash", last name "Agent"; `services/src/domain/identity.rs:673-678`), and bot comments are labelled "AI agent: Pi Dash Agent" in prompts (`prompting/context.rs:530`). The native agent would share the name | Keep "Pi Dash Agent" for the native agent. In code, call the bot the "agent bot user" (`AGENT_BOT_USERNAME`, and so on) with its stored values unchanged. Decide separately whether the bot's display name should change; that is a data and Django change |
| 2 | Code spelling for the native agent | `pidash_agent` / `PidashAgent` (6.1) |
| 3 | Server-written sentences pinned to Django by contract tests still say "engine" and "built-in engine" | Leave them in this work. The new frontend maps reason codes to its own text, so most users never see the server sentence. Change them when Django is retired or changed too |
| 4 | The project payload still emits a deprecated `cloud_agent` executor option row | Emit a static unavailable row with no settings behind it, so the legacy frontend and the Django-parity tests keep passing. Drop the row when the legacy frontend goes |
| 5 | Add CLI aliases such as `pidash agent …` and `--pidash-agent` next to `engine` and `--engine` | Not in this work. The old verbs must keep working regardless; aliases can follow |
| 6 | Six Tauri `daemon_engine_*` commands have no caller in the new frontend, and the hidden `pidash __managed` command family has no caller outside tests | Leave both. Older desktop builds may use them; removing them is a separate clean-up |
| 7 | The retired variant's name and label (`AgentExecutorKind::CloudAgent`, label "Pi Dash Cloud Agent") | Rename the Rust variant to `RetiredCloudRun`; keep the label, which is pinned to Django's choices |

## 12. Not part of this spec, but related

- **A dedicated worker for chat turns.** The retired path had its own queue and worker. The long-running job that needs that isolation is `assistant.run_turn`. Pointing the generic `--queue` option at chat turns is a follow-up.
- **OpenHub app tools in chat.** The chat engine cannot attach the OpenHub gateway's app toolset yet (NEWPIDASH-193).
- **A Responses-API lane**, so Codex-lane users can use the cloud agent.
- **Sharing tool code between the assistant's tools and the MCP tools** (NEWPIDASH-190), which should target `api/src/turn/tools_local*.rs`, not the retired run tools.
- **Django and the legacy frontend.** Their names change when those systems are next touched or retired.
