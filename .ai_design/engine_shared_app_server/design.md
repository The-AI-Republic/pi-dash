# Design: Pi Dash Agent chat on one shared engine app-server

> Directory: `.ai_design/engine_shared_app_server/`
>
> Two changes, shipped together:
>
> 1. **Cloud runner chat is only for the Pi Dash Agent.** Runners backed by the
>    user's own agent (Codex, Claude Code, Cursor, …) no longer offer chat.
> 2. **One always-on engine app-server per daemon.** The daemon keeps a single
>    `pidash-agent-engine app-server` child process running and serves every
>    Pi Dash Agent chat from it, desktop local chat and cloud chat alike, as
>    threads inside that one process.
>
> **AgentRun is out of scope.** Every run, on every runner type including engine
> runners, keeps its own agent process, its own `PIDASH_RUN_ID` and its own
> containment unit, exactly as today.

## 1. Problem

### 1.1 Runner chat is slow and rarely used

Every chat session starts its agent cold. On the cloud path
(`runner/src/daemon/supervisor.rs`, `handle_warm` at `:3625`) and the desktop
path (`runner/src/ipc/chat.rs`, `handle_warm` at `:428`), the daemon:

1. spawns a new agent process (`AgentBridge::spawn_from_config_with_resume`),
2. for Codex and the engine: sends `initialize`, then `thread/start` or
   `thread/resume`,
3. only then can a turn run.

Warm-up (`useWarmUp` in `apps/web_new/src/features/runners/chat/`) hides part
of this behind the time the user spends typing, but measured cold starts on
normal runners are long enough to make chat feel broken. A session cleaned up
after idling (desktop: more than 8 live, or idle for 10 minutes,
`ipc/chat.rs:75-79`) pays the full cold start again when reopened.

For runners on the user's own agent, chat also adds little: users seldom need
to talk to a runner directly; they work through issues and runs. It still
costs a lot of code: a cross-lane rule in the supervisor (a chat turn blocks a
task assignment and a task blocks chat, `supervisor.rs:2770-2805`, `:2945-2970`,
`:3020-3045`), a chat runtime per runner, and a matching cloud surface.

### 1.2 The engine is built to be shared, but we start one per chat

The Pi Dash Agent is the open-source Codex binary, renamed
`pidash-agent-engine` (`runner/src/pidash_agent/mod.rs`). Codex app-server is
built for one long-lived process serving many conversations:

- initialize once per connection, then `thread/start` / `thread/resume` per
  conversation (app-server README, "Lifecycle Overview");
- `thread/start` and `thread/resume` take a per-thread `cwd`, `model`,
  `approval_policy`, `developer_instructions` and a `config` override map
  (`app-server-protocol/src/protocol/v2/thread.rs`);
- `turn/start` takes per-turn `cwd`, model and approval overrides;
- every notification and approval request carries `threadId`;
- `thread/unsubscribe` lets the server unload a thread after it has been idle
  (60 s by default, `thread_unload_delay_secs`);
- Codex itself ships `codex app-server daemon` so clients attach to one running
  server instead of starting their own (`app-server-daemon/README.md`).

Today the daemon instead starts one engine process per chat session, so every
chat pays process startup plus `initialize`.

## 2. Decisions

| #   | Decision                                                                                                                                      | Status  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| D1  | Cloud runner chat is offered only for engine runners (`Runner.provisioning = desktop_bundled`, `Runner.is_engine`).                           | decided |
| D2  | The daemon runs one always-on engine app-server as a child process and serves all Pi Dash Agent chat (desktop and cloud) from it.             | decided |
| D3  | AgentRun is unchanged for every runner type: one agent process per run, `PIDASH_RUN_ID`, per-run containment.                                 | decided |
| D4  | Cloud engine chat no longer runs in the runner's project folder. MVP: a default folder, `<data_dir>/engine/chat` (see O1).                    | decided |
| D5  | Choosing the folder for a cloud chat is a later feature. The session already has a `cwd` field; the cloud ignores it today (`normalize_cwd`). | decided |
| D6  | Cloud chat keeps reaching the daemon through the engine runner's existing long-poll and `chat_*` messages. No new transport.                  | decided |

## 3. Part A: remove cloud chat for own-agent runners

Enforced at every layer, so an old client cannot reach the removed path.

### 3.1 Web (`apps/web_new`)

- Show the chat entry and the chat route only for engine runners. An own-agent
  runner row opens its detail/overview, never `/runners/chat/:runnerId`.
- A direct visit to the chat route for an own-agent runner shows a short
  "chat is available on the Pi Dash Agent" state instead of a composer.
- Existing sessions on own-agent runners: hidden from the history list (see
  O3).

### 3.2 Cloud (Python `apps/api/pi_dash/runner/views/chat.py` and Rust `rust-api/crates/api/src/runner_runs/chat.rs`)

- Session create, warm and message send refuse a non-engine runner with
  `409 {"error": "chat_not_supported"}`. Both backends change together and keep
  identical bodies (the Rust port is byte-compatible with Python).
- The runner-facing chat endpoints (`/api/v1/runner/chat/...`) stay: engine
  runners still use them.
- The chat tables, SSE stream (`/api/runners/chat/sessions/<id>/events/`) and
  sweeps stay, since engine runners still use them.

### 3.3 Runner (`runner/src/daemon/supervisor.rs`)

- An own-agent runner answers `ChatUserMessage` with
  `ChatFailed { code: "chat_not_supported" }` and `ChatWarm` with a
  `chat_warm_skipped { reason: "chat_not_supported" }` event, as a defensive
  check behind the cloud refusal.
- The chat runtime, `current_chat` and the cross-lane rules are removed from
  own-agent runners. Their assign path keeps only the single-run guard.

## 4. Part B: the shared engine app-server

### 4.1 Shape

```
                 pidash daemon
 ┌──────────────────────────────────────────────────────────┐
 │                                                          │
 │  IPC server ── desktop ChatWarm/Send/Cancel/Close/Decide ─┐
 │                                                          ││
 │  engine runner instance(s) ── cloud chat_* over long-poll ┤
 │                                                          ││
 │                                     EngineChatServer  ◀──┘│
 │                                     (one per daemon)      │
 │                                        │ stdio JSON-RPC   │
 └────────────────────────────────────────┼──────────────────┘
                                          ▼
                         pidash-agent-engine app-server  (child process)
                         thread A (desktop)  thread B (cloud)  thread C …

 AgentRun on an engine runner (unchanged):
   supervisor ── spawn ──▶ pidash-agent-engine (own process, own containment)
```

`EngineChatServer` is a new daemon component that owns the child process and
the one JSON-RPC connection to it. Desktop chat and cloud chat both call it;
neither spawns an engine anymore.

### 4.2 Lifecycle

- **Start:** when the engine can run: binary located (`pidash_agent::locate`),
  engine credential present, model profile available
  (`PidashAgentManager`), engine not paused. The daemon spawns the child with
  the engine's `CODEX_HOME` and auth helper (as `pidash_agent::agent::launch`
  does today) and sends `initialize` immediately, so the first chat finds the
  server ready.
- **Stop:** on engine sign-out, credential revoke, pause, or daemon shutdown.
  Running turns are interrupted and their sessions get `chat_failed` with a
  reason (`engine_signed_out`, `engine_paused`, `daemon_stopping`).
- **Crash:** the daemon restarts the child with backoff. Turns in progress fail
  (`chat_failed { code: "engine_restarted" }`); sessions keep their thread id
  and resume on their next warm or send (`thread/resume`).
- **Model profile change:** see O4. MVP: wait until no turn is running, then
  restart the child; new and resumed threads read the new config.

One server per daemon. The engine uses one `CODEX_HOME` per account
(`<daemon data>/engine/<account>/codex-home`); a daemon has one engine
account today. If a daemon ever holds several engine accounts, the component
holds one child per account.

### 4.3 Sessions and threads

- `EngineChatServer` keeps `chat_session_id → thread_id` plus per-session state
  (active turn id, event sequence, approval sink).
- **Warm** = make sure the server is up, then `thread/resume` (known thread id)
  or `thread/start` (new). No process spawn. Returns the thread id as today, so
  `ChatStarted` / `chat_warmed` and the cloud's `local_thread_id` are
  unchanged.
- **Send** = warm if needed, then `turn/start` on the thread.
- **Cancel** = `turn/interrupt`. **Close** = `thread/unsubscribe` and drop the
  session.
- **Idle sessions:** `thread/unsubscribe` after an idle period; the server
  unloads the thread after its own delay. Reopening is a `thread/resume` inside
  the running process. This replaces the 8-process cap and 10-minute reaper in
  `ChatRegistry`.

### 4.4 Routing

One connection carries every thread, so the component reads every frame and
routes by `threadId`:

- notifications (`turn/*`, `item/*`, deltas) → that session's event sink:
  - desktop sessions: the IPC stream for that request (as `ipc/chat.rs` does
    today);
  - cloud sessions: `ClientMsg::ChatEvent` / `ChatMessageCompleted` / … through
    the engine runner's outbox, exactly as today.
- approval requests → the session's approval path:
  - desktop: the local approval router and per-session approval mode
    (RUN-034/035), unchanged for the user;
  - cloud: `ClientMsg::ChatApprovalRequest`, answered by `ServerMsg::ChatDecide`.
- Each thread's approval policy and sandbox are set on `thread/start` /
  `thread/resume`, so desktop and cloud threads can differ.

### 4.5 Desktop local chat

- The IPC requests stay the same (`ChatWarm`, `ChatSend`, `ChatCancel`,
  `ChatClose`, `ChatDecide`, `runner/src/ipc/server.rs:437-519`); the desktop
  app and `localTransport.ts` need no change.
- `PidashAgentChatProfile` (`runner/src/ipc/pidash_agent_chat.rs`) stops
  spawning a bridge per session and calls `EngineChatServer` instead.
- The chat folder rule (`resolve_engine_chat_folder`) is unchanged.

### 4.6 Cloud engine chat

- Messages still arrive on the engine runner's long-poll
  (`ServerMsg::ChatWarm`, `ChatUserMessage`, `ChatCancel`, `ChatClose`,
  `ChatDecide`) and leave through its outbox (D6). The cloud protocol is
  unchanged.
- The engine runner's supervisor hands them to the daemon's `EngineChatServer`
  instead of running its own chat runtime.
- **Folder (D4):** `<data_dir>/engine/chat`, created by the daemon (private,
  like the rest of `data_dir`). `resolve_chat_workspace` stops using
  `workspace.working_dir` for engine chat and resolves this folder through the
  same `resolve_engine_chat_folder` rule desktop chat uses. The session's `cwd`
  stays empty in MVP (D5).
  - `data_dir` is the daemon's per-machine data folder (`util/paths.rs`):
    `~/.local/share/pidash` on Linux,
    `~/Library/Application Support/so.pidash.pidash` on macOS,
    `%APPDATA%\pidash\pidash\data` on Windows, or the `PIDASH_DATA_DIR` /
    `--data-dir` override (desktop and CI installs).
  - `engine/` already holds `engine/<account>/codex-home` and the default
    engine task root `engine/workdirs` (`pidash_agent/workdir.rs`);
    `engine/chat` is their sibling.
- **No more busy coupling:** the chat no longer shares the runner's project
  folder, so for engine runners:
  - chat is accepted while that runner has a run in progress (no
    `runner_busy` for chat);
  - a task assignment is no longer rejected while a chat turn is active.

  This is safe because `engine/chat` is outside every task folder, so a chat
  cannot edit a running task's files (O1).

- Several cloud chats on one engine runner can be open at once; each is its own
  thread. The cloud's per-session `chat_turn_active` rule stays.

## 5. What does not change

- AgentRun on every runner type (D3), including engine runners: own process,
  `PIDASH_RUN_ID` (`util/shell.rs:81-93`), containment
  (`containment::create_run_group`, `run_cleanup`), shutdown at run end.
- The cloud chat protocol, tables, SSE stream and sweeps.
- The desktop IPC chat protocol and the desktop app.
- Own-agent runners' task behaviour.

## 6. Open decisions

**O1. Default chat folder. Resolved: `<data_dir>/engine/chat`.** The home
directory (`~`) was considered first and rejected: `resolve_engine_chat_folder`
refuses a folder _inside_ a task folder, but `~` _contains_ every task folder,
and Codex's default `workspace-write` sandbox makes the thread's `cwd`
writable, so a chat in `~` could edit a task worktree while a run is active,
and edit dotfiles. `<data_dir>/engine/chat`:

- is outside every task folder, so the existing folder rule accepts it
  unchanged;
- keeps `workspace-write` safe: only `engine/chat` is writable, not task
  folders, dotfiles or the engine's `codex-home`;
- follows the data-dir override, so isolated desktop and CI installs each get
  their own.

Trade-off: it is a hidden location, so files a chat creates are hard for the
user to find. Folder selection (O2) addresses that later.

**O2. Folder selection.** Later: the user picks the folder per cloud chat. The
cloud passes it in the existing `cwd` field; the daemon validates it with the
same rule as desktop chat.

**O3. Existing chat history on own-agent runners.** Hide it (recommended), keep
it readable but closed, or delete it in a migration.

**O4. Picking up a new model profile.** Restart when idle (MVP), or verify that
Codex reads updated `config.toml` for new threads without a restart, which
would avoid restarts entirely (spike S2).

**O5. Concurrency limit.** A cap on simultaneous running turns in the shared
server (all sessions together), with `chat_failed { code: "engine_busy" }`
beyond it. Proposed default: 4.

## 7. Risks

- **One process for all chats.** A crash interrupts every chat on the machine.
  Mitigated by automatic restart and `thread/resume`; chats lose only the turn
  in progress.
- **`CODEX_HOME` shared with per-run engine processes.** The shared server and
  each AgentRun's engine use the same `CODEX_HOME`. They already do today
  (desktop chat and runs), but the shared server lives much longer. Codex
  allows only one process to hold a paginated thread open for writing; chat
  and run threads are different threads, so this does not collide. `thread/start`
  with a writable `cwd` marks the project trusted in `config.toml`, which the
  daemon also rewrites on profile refresh (spike S3).
- **Version coupling.** The engine is pinned (`runner/ENGINE_VERSION`). The
  features this design uses (`thread/unsubscribe`, per-thread `config`) must
  exist in the pinned version (spike S1).

## 8. Spikes

- **S1.** Confirm on the pinned engine version: several threads with turns
  running at the same time in one app-server; per-thread `cwd`, approval policy
  and sandbox; `thread/unsubscribe` unloading.
- **S2.** Does a new or resumed thread pick up a changed `config.toml` without
  restarting the process?
- **S3.** Concurrent writers to `CODEX_HOME/config.toml` (trust marking by the
  server, profile rewrite by the daemon, per-run engines).
- **S4.** Measure: time to first token for a new desktop chat and a new cloud
  engine chat, before and after.

## 9. Delivery

1. **Part A:** web gating, cloud refusal (Python and Rust), runner defensive
   refusal, supervisor cleanup for own-agent runners. Independent of Part B.
2. **Spikes S1–S3.**
3. **`EngineChatServer`** in the daemon, with desktop local chat moved onto it.
4. **Cloud engine chat** moved onto it, the `<data_dir>/engine/chat` folder, and the
   busy coupling removed for engine runners.

## 10. Acceptance

- An own-agent runner shows no chat in the web app, the cloud refuses chat for
  it with `chat_not_supported`, and its runs behave as before.
- With the engine available, the daemon has exactly one
  `pidash-agent-engine app-server` child that is not attached to a run.
- Opening a new desktop chat starts no engine process; its first reply starts
  measurably sooner than today (S4).
- A cloud chat with an engine runner runs in `<data_dir>/engine/chat`, cannot
  write outside it, is accepted while that runner
  has a run in progress, and does not block the runner's next task.
- Killing the shared child fails only the turns in progress; reopened sessions
  continue their conversation.
- An AgentRun on an engine runner still starts its own engine process, carries
  `PIDASH_RUN_ID`, and is fully cleaned up at the end.
