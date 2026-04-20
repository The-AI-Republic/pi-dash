# Pi Dash Runner — MVP Design

This document specifies the MVP runner for Pi Dash: a **thin local connector** that accepts tasks assigned by the Pi Dash cloud and invokes Codex locally to execute them. The runner performs **no orchestration** — scheduling, retries, matching, prompt composition, and "done" interpretation all live in the cloud.

Companion docs in this directory:

- `github-runner-architecture.md` — reference for how GHA's runner works (blueprint).
- `tui-design.md` — runner's built-in TUI.

---

## 1. Scope

### In scope

- Register with Pi Dash cloud using a one-time token → obtain long-lived runner credentials.
- Maintain a persistent outbound WebSocket to the cloud; send heartbeats; reconnect with backoff.
- Accept one task at a time; bridge the cloud's task message to `codex app-server`.
- Relay approval requests from Codex up to the cloud (and to the local TUI); accept decisions from either surface.
- Report lifecycle events to the cloud (`run_started`, `approval_request`, `run_completed` with Codex's structured "done" payload, `run_failed`, `run_cancelled`, `run_awaiting_reauth`).
- Keep Codex event stream **local only** (not streamed to cloud). Persist per-run history on disk.
- Built-in TUI (Ratatui) that attaches to the daemon over a local socket.
- First-run validation: Codex installed + logged in, git configured with working credentials.

### Out of scope for MVP

- Orchestration logic (scheduling, retry, matching) — cloud owns this.
- Prompt composition from work-item fields — cloud owns this.
- Opening PRs, running tests, interpreting "done" — Codex emits a structured signal per the cloud's prompt, runner passes it up, cloud decides.
- Multiple runners per machine — one runner per machine in MVP.
- Label/capability matching — each runner has a globally-unique ID; cloud assigns to an online idle runner owned by the user.
- Codex sandbox mode enforcement beyond defaults — deferred.
- Streaming Codex event deltas to cloud — local only in MVP; cloud can fetch history later.
- Cross-task workspace isolation — single shared working directory.
- Windows support — v2.
- Native tray icon — post-v1.

---

## 2. Committed decisions (from scoping)

| #   | Topic                        | Decision                                                                                                                                                                                                                                                                       |
| --- | ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | Working directory            | Runner config specifies working dir. Default `$TMPDIR/.pidash/`. If dir has a git repo → use it. If not → request repo URL from cloud, `git clone` into dir. Clone-auth failure is reported. Every task runs Codex against this dir.                                           |
| 2   | Concurrency                  | One task at a time per runner.                                                                                                                                                                                                                                                 |
| 3   | Assignment                   | One runner instance per dev machine. Globally unique runner ID. Cloud assigns to an online idle runner owned by the user. No label matching.                                                                                                                                   |
| 4   | Prompt construction          | Cloud renders; runner receives a ready-to-use prompt string. Runner never composes prompts.                                                                                                                                                                                    |
| 5   | "Done" criteria              | Cloud's prompt instructs Codex to emit structured data as a done signal. Runner forwards that data verbatim. Cloud decides what happens next.                                                                                                                                  |
| 6   | Approval policy              | Auto-approve read-only allowlist; ask for everything else; never auto-approve destructive ops / writes outside workspace / git push / non-allowlisted network. Editable per-runner in TUI.                                                                                     |
| 7   | Platforms v1                 | macOS arm64, macOS x64, Linux x64. Linux arm64 in v1.1. Windows v2.                                                                                                                                                                                                            |
| 8   | Failure recovery             | WS drop → reconnect + report state. Runner crash → Codex dies, mark failed on restart. Codex crash → attempt `thread/resume` once, else failed. Laptop sleep → reconnect + `thread/resume` on wake. Codex 401 → `awaiting_reauth`, prompt user via TUI/web, resume on success. |
| A   | Git credentials              | Runner uses user's existing git config. First-run onboarding validates with a dry-run `git ls-remote` against the user's primary repo host.                                                                                                                                    |
| C   | Cloud-initiated cancellation | Cloud sends `cancel` → runner sends `turn/interrupt` to Codex → wait for graceful stop (SIGKILL fallback after N seconds) → runner reports `run_cancelled`.                                                                                                                    |
| D   | Event privacy                | Codex `item/*` deltas stay **local** (persisted to run history file). Only lifecycle + approval + final done signal go to cloud.                                                                                                                                               |
| E   | Idle behavior                | Heartbeats only. No periodic doctor checks. No pre-warm of Codex.                                                                                                                                                                                                              |

---

## 3. Architecture

```
┌─────────────────────── dev laptop ─────────────────────────────────┐
│                                                                     │
│   pidash  (daemon, long-lived)                        │
│   ┌─────────────────────────────────────────────────────────────┐  │
│   │  Cloud WS client  ◄─── outbound WSS on 443 ────►            │  │
│   │                                                             │  │
│   │  State machine (one active run at a time)                  │  │
│   │                                                             │  │
│   │  Codex bridge  ◄── stdio ──►  codex app-server (subprocess)│  │
│   │                                                             │  │
│   │  Working dir manager         Approval router               │  │
│   │                                                             │  │
│   │  Local IPC server (Unix socket)                             │  │
│   │  History writer (JSONL)      Config file (TOML)             │  │
│   └─────────────────────────────────────────────────────────────┘  │
│            ▲                                                        │
│            │ Unix socket                                            │
│   pidash tui  (Ratatui, ad-hoc client)                │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
                        │  WSS  │
                        ▼       ▼
                 ┌──────────────────────┐
                 │  Pi Dash cloud  │
                 │  (Django + Node WS)   │
                 └──────────────────────┘
```

Key properties:

- Single long-lived outbound WebSocket — NAT-friendly, the one and only public surface.
- No inbound listeners on the laptop except a local Unix socket (0600, user-only).
- Codex is spawned on demand per task; killed and cleaned up per task.
- TUI is a separate process; it starts and stops freely without affecting the daemon.

---

## 4. Runner lifecycle

### 4.1. Install

Distribution: one static binary per target triple. Users install via:

- Homebrew tap: `brew install pi-dash/tap/runner`
- Direct download: `curl -fsSL https://... | sh`
- Linux: `.deb` / `.rpm`
- Future: Windows MSI (v2)

Install does not require root; binary lives in `$HOME/.local/bin/pidash` by default.

### 4.2. First-run configure

```
pidash configure \
  --url https://cloud.pidash.so \
  --token <ONE_TIME_CODE> \
  [--name my-laptop]
```

Sequence:

1. POST `{runner_name, os, arch, version}` + token to cloud registration endpoint.
2. Cloud returns long-lived `runner_id` (global UUID) + `runner_secret` (bearer token).
3. Write `config.toml` and `credentials.toml` with file mode 0600.
4. **Validate environment** (on-install doctor):
   - Codex binary: which `codex` resolves; run `codex --version`.
   - Codex auth: run `codex whoami` (or equivalent). Refuse to complete setup if unauthenticated.
   - Git config: verify `user.name` and `user.email` set, run `git ls-remote <any configured repo>` dry-run. Warn if git creds look missing.
5. Print a success line with the runner's global ID and instruction to run `service install` or `start`.

If TUI is used instead, the first-run wizard in `tui-design.md` performs steps 1–5 interactively.

### 4.3. Service install

```
pidash service install   # systemd user unit / launchd plist
pidash service start
pidash service status
pidash service stop
```

- **Linux**: generate a `~/.config/systemd/user/pidash.service`, `systemctl --user daemon-reload`, enable with `linger` optional.
- **macOS**: generate a `~/Library/LaunchAgents/so.pidash.daemon.plist`, load with `launchctl bootstrap gui/$UID`.

### 4.4. Run loop (simplified)

```
connect_to_cloud():
    open WSS
    send Hello { runner_id, version, os, arch, status: Idle }
    expect Welcome

main_loop():
    spawn heartbeat_task    # sends Heartbeat every 25s
    spawn ipc_server_task   # local Unix socket for TUI
    while true:
        msg = read_ws()
        match msg:
            Assign(task)   -> handle_assign(task)
            Cancel(run_id) -> handle_cancel(run_id)
            Decide(dec)    -> handle_decision(dec)
            ReauthHint     -> notify_user()

on_ws_drop():
    jittered_backoff()
    reconnect()
    on success: resend current run state if a run is in-flight
```

### 4.5. Handling one task (happy path)

```
handle_assign(task):
    send Accept { run_id: task.run_id }
    resolve_workspace()
        ├─ if config.working_dir is a valid git repo: use it
        └─ else:
            request RepoInfo from cloud (already carried in Assign)
            run `git clone <url> <working_dir>` using user's git creds
            on failure (auth or network): send RunFailed{reason: workspace_setup}
    spawn codex app-server (--cwd = working_dir)
    codex: initialize -> thread/start (no resume) -> turn/start { prompt: task.prompt }
    send RunStarted { run_id, thread_id, started_at }
    loop on codex events:
        item/commandExecution/requestApproval -> approval_flow()
        item/fileChange/requestApproval        -> approval_flow()
        (every other item/* event)             -> write to local history; do NOT send to cloud
        turn/completed { conclusion: success, done_payload } -> send RunCompleted; break
        turn/completed { conclusion: failed }  -> send RunFailed; break
    cleanup:
        close thread, terminate app-server
        persist final history record
        set state = Idle
```

### 4.6. Approval flow

```
approval_flow(req):
    check policy:
        if req matches auto-approve allowlist: respond Accept; return
        if req matches auto-decline list:      respond Decline; return
    persist ApprovalRequest { pending }
    notify TUI (via IPC) and cloud (via WS)
    wait for Decide { approval_id, decision } from either:
        - TUI subscriber (local Unix socket)
        - Cloud WS
    deduplicate: first responder wins; record source
    send decision back to Codex via app-server
    persist ApprovalRequest { resolved }
```

### 4.7. Shutdown

- `SIGTERM` → stop accepting new assignments; if a run is active, attempt graceful Codex `turn/interrupt`, wait up to N seconds, then SIGKILL; close WS; flush history; exit.
- `runner remove --token <REMOVAL_TOKEN>` → call cloud `deregister`, delete credentials and config, exit.

---

## 5. Working directory & git setup

This is the one piece of business logic the runner owns. Detailed behavior:

### 5.1. Configuration

```toml
# ~/.config/pidash/config.toml
[workspace]
working_dir = "/var/folders/.../T/.pidash"   # default: $TMPDIR/.pidash
```

Override via CLI: `pidash configure --working-dir /path/to/repo`.

### 5.2. On assignment

```
resolve_workspace(task):
    dir = config.workspace.working_dir
    ensure dir exists (mkdir -p)
    if is_git_repo(dir):                          # dir/.git exists
        log("using existing repo at {dir}")
        return dir
    if dir_is_empty(dir):
        git_clone(task.repo_url, dir)              # uses user's git creds
        return dir
    return Err(NonEmptyNonRepo)                   # refuse to operate
```

### 5.3. Clone failures

`git clone` can fail for:

- Network unreachable → `run_failed { reason: network }`.
- Auth rejected (SSH key, HTTPS creds) → `run_failed { reason: git_auth, detail: <stderr> }`.
- URL invalid → `run_failed { reason: invalid_repo_url }`.

All stderr is captured verbatim in the failure report. Cloud surfaces this to the user in the work item UI.

### 5.4. Reused state between tasks

Runner does **not** reset the working directory between tasks. Dirty trees, stray branches, cached build artifacts all persist. This is intentional:

- MVP concurrency is 1, so there's no in-flight overlap.
- Reusing the dir makes large repos fast (no re-clone, no re-install of deps).
- Hygiene is the **prompt's** responsibility. The cloud instructs Codex (via the rendered prompt) to reset/stash/checkout as appropriate before starting work.

The runner surfaces current git state (branch, dirty?) in a pre-run `workspace_state` field included in `Accept` so the cloud can decide to inject prompt preamble about cleanup if it cares.

### 5.5. Caveat to document in onboarding

If `working_dir` is inside `$TMPDIR` on Linux (often `/tmp`, which may be `tmpfs`), the clone is lost on reboot and re-cloning happens on the next task. For large repos, users should override `working_dir` to a durable path.

---

## 6. Data model

### 6.1. On-disk layout

```
~/.config/pidash/
├── config.toml          # user-editable config
└── credentials.toml     # runner_id + runner_secret (0600)

~/.local/share/pidash/
├── history/
│   ├── runs/
│   │   └── <run_id>.jsonl      # one JSONL per run; all Codex events + lifecycle
│   └── runs_index.json         # small index: run_id, work_item_id, status, timestamps
├── logs/
│   └── YYYY-MM-DD.log
└── pid                         # daemon PID file
```

### 6.2. Config schema

```toml
version = 1

[runner]
name = "my-laptop"
cloud_url = "https://cloud.pidash.so"
# runner_id + secret are in credentials.toml

[workspace]
working_dir = "/var/folders/.../T/.pidash"

[codex]
binary = "/usr/local/bin/codex"      # auto-detected; overridable
model_default = "gpt-5-codex"

[approval_policy]
auto_approve_readonly_shell = true
auto_approve_workspace_writes = false
auto_approve_network = false
allowlist_commands = ["ls", "cat", "pwd", "git status", "git diff", "git log", "git branch"]

[logging]
level = "info"
retention_days = 14
```

### 6.3. Credentials

```toml
runner_id = "r_01HY..."        # server-assigned UUID, global
runner_secret = "rns_..."      # bearer, used in WS Authorization header
issued_at = "2026-04-18T12:34:56Z"
```

### 6.4. Run history record (JSONL)

Each run is one file: `history/runs/<run_id>.jsonl`. Line-delimited JSON. Lines are:

- `header` (first line) — task metadata, start time.
- `codex_event` — every `item/*` and `turn/*` notification from app-server, verbatim.
- `approval` — approval requested + resolved.
- `lifecycle` — state transitions (started, awaiting_approval, awaiting_reauth, cancelled, failed, completed).
- `footer` (last line) — final status + done payload (if any) + end time.

Rationale for JSONL over SQLite in v1: simpler, zero schema migrations, `tail -f` works for debugging. Migrate to SQLite in v2 if query needs grow.

---

## 7. Cloud ↔ runner protocol

Transport: WebSocket Secure (WSS) on 443. Messages are JSON frames. Every message has `{ "type": "...", "v": 1, ... }`.

Authorization: `Authorization: Bearer <runner_secret>` on the initial HTTP upgrade. `runner_id` is derived from the token server-side and echoed in the welcome.

### 7.1. Lifecycle messages

**Runner → Cloud:**

| Type                  | Shape                                                        | Meaning                                                                                    |
| --------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `hello`               | `{ runner_id, version, os, arch, status: "idle" \| "busy" }` | First frame after connect                                                                  |
| `heartbeat`           | `{ ts }`                                                     | Every 25s                                                                                  |
| `accept`              | `{ run_id, workspace_state: { branch, dirty } }`             | Accepting an assignment                                                                    |
| `run_started`         | `{ run_id, thread_id, started_at }`                          | Codex thread established                                                                   |
| `approval_request`    | `{ run_id, approval_id, kind, payload, reason }`             | Forwarded from Codex, needs human                                                          |
| `run_awaiting_reauth` | `{ run_id }`                                                 | Codex returned 401                                                                         |
| `run_completed`       | `{ run_id, done_payload, ended_at }`                         | Codex reported success; `done_payload` is the structured object Codex was prompted to emit |
| `run_failed`          | `{ run_id, reason, detail }`                                 | `reason ∈ { workspace_setup, git_auth, network, codex_crash, internal }`                   |
| `run_cancelled`       | `{ run_id, cancelled_at }`                                   | Acknowledges a cancel                                                                      |
| `bye`                 | `{ reason }`                                                 | Graceful disconnect (service stopping)                                                     |

**Cloud → Runner:**

| Type          | Shape                                                                                                      | Meaning                                                |
| ------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| `welcome`     | `{ server_time, protocol_version }`                                                                        | Initial reply                                          |
| `assign`      | `{ run_id, work_item_id, prompt, repo_url, repo_ref?, expected_codex_model?, approval_policy_overrides? }` | New task                                               |
| `cancel`      | `{ run_id, reason }`                                                                                       | Abort the active run                                   |
| `decide`      | `{ run_id, approval_id, decision: "accept" \| "decline" \| "accept_for_session", decided_by }`             | Answer to an approval                                  |
| `config_push` | `{ approval_policy }`                                                                                      | Optional cloud-side policy updates (deferred past MVP) |
| `ping`        | `{ ts }`                                                                                                   | Optional; runner echoes with heartbeat                 |

### 7.2. Flow diagrams

**Happy-path task:**

```
runner                                     cloud
  │                                          │
  │ ──────── hello ─────────────────────────► │
  │ ◄──────── welcome ─────────────────────── │
  │                                          │
  │ ◄──────── assign { run_id, prompt,… } ─── │
  │ ─────── accept { workspace_state } ─────► │
  │ ─── (git clone if needed, spawn codex) ─  │
  │ ─────── run_started ──────────────────── ► │
  │                                          │
  │ ─────── approval_request ──────────────► │   (if Codex asks)
  │ ◄──────── decide { accept } ──────────── │
  │                                          │
  │ ─────── run_completed { done_payload } ► │
```

**Cancel:**

```
  │ ◄──────── cancel { run_id } ──────────── │
  │ (runner.turn_interrupt → codex)         │
  │ (grace 10s, then SIGKILL app-server)    │
  │ ─────── run_cancelled ────────────────► │
```

**Disconnect during run:**

```
  × WS drops
  (codex keeps running locally)
  (jittered backoff: 1–10s → 2–20s → … cap 60s)
  │ ──────── hello { status: "busy" } ─────► │
  │ ──────── run_resumed { run_id, thread_id, elapsed } ─► │   (extend of hello)
  │ ◄──────── welcome ─────────────────────── │
  (continue streaming approvals/completion)
```

### 7.3. Backoff

Reconnect on disconnect: `sleep = min(60, 2^n + rand(0, n))` seconds where `n` is the consecutive-failure count, capped at 60s, reset on success.

---

## 8. Codex bridge

Runner talks to `codex app-server` over stdio (default). WebSocket mode (`codex app-server --listen ws://127.0.0.1:<port>`) is a v1.1 option if stdio proves limiting.

### 8.1. Subprocess lifecycle

```
per-task:
    binary = config.codex.binary
    args   = ["app-server"]
    env    = current + any CODEX_* overrides
    cwd    = config.workspace.working_dir

    spawn with tokio::process::Command
    wire:
        child.stdin  ← JSON-RPC requests
        child.stdout → JSON-RPC responses + notifications (line-delimited)
        child.stderr → drained to runner log file
```

### 8.2. JSON-RPC methods invoked

Phase ordering within one task:

1. `initialize { clientInfo: { name: "pidash", version } }` → wait for `initialize` response.
2. Send `initialized` notification.
3. `account/read` — confirm auth state. If unauthenticated → `run_awaiting_reauth`.
4. `thread/start { cwd, model?, sandboxPolicy: "workspace-write", approvalPolicy: "on-request" }` → capture `thread_id`.
5. `turn/start { input: [{ role: "user", content: task.prompt }], model?, effort? }`.
6. Listen for notifications:
   - `item/agentMessage/delta`, `item/reasoning/textDelta`, `item/commandExecution/outputDelta`, `item/fileChange/outputDelta`, `turn/diff/updated`, `turn/plan/updated`, `thread/tokenUsage/updated` → write to **local history only**.
   - `item/commandExecution/requestApproval`, `item/fileChange/requestApproval` → run through approval flow (section 9).
   - `item/completed`, `item/started` → local history only.
   - `turn/completed` → parse conclusion + extract done_payload from the final `item/agentMessage/completed` content (per the cloud's prompt contract).
7. Close the thread, terminate the app-server subprocess.

### 8.3. Resume on reconnect

If the WS drops mid-run but Codex is still alive:

- On reconnect, runner sends `hello { status: "busy" }` + `run_resumed { run_id, thread_id }`.
- Cloud marks the run as `reconnected`, trusts the runner's state.
- Any approvals/events that occurred during disconnect are replayed up if they're still pending (runner buffers them).

If Codex died too:

- Runner attempts `thread/resume { threadId }` **once** on a fresh app-server.
- If resume fails (Codex returns `thread_not_found` or similar), runner reports `run_failed { reason: codex_crash }`.

---

## 9. Approval flow

### 9.1. Policy evaluation

On `requestApproval`:

```
evaluate(req):
    if is_destructive(req):                    # rm -rf, writes outside workspace, sudo
        return Ask                             # never auto-approve
    if matches allowlist_commands(req.command):
        return AutoAccept
    if req.kind == commandExecution and req.command is read-only shell:
        if config.auto_approve_readonly_shell:
            return AutoAccept
    if req.kind == fileChange and req.path inside working_dir:
        if config.auto_approve_workspace_writes:
            return AutoAccept
    return Ask
```

### 9.2. Ask path

1. Persist a pending `ApprovalRequest` locally (JSONL + in-memory index).
2. Publish to IPC subscribers (TUI) and WS (cloud). Both surfaces can answer.
3. Wait for the **first** `decide` to arrive (from either).
4. Send the decision back to Codex (`item/commandExecution/requestApproval` response with `decision`).
5. Record which surface answered and who (`decided_by`).
6. A late second `decide` is ignored with a log line.

### 9.3. Decision semantics

- `accept` — Codex proceeds with this one action.
- `accept_for_session` — runner remembers for the rest of this thread and auto-answers subsequent matching requests.
- `decline` — Codex sees it as denied; usually continues with an alternative or stops.
- Cancel of the run happens via the separate `cancel` message, not through an approval response.

---

## 10. Failure recovery (summary table)

| Scenario                       | Runner action                                                                                                     | Reported state             |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------- | -------------------------- |
| WS drops, Codex alive          | Jittered backoff, reconnect, `hello` with `busy`, `run_resumed` with state                                        | `reconnected` (cloud-side) |
| WS drops, Codex dies           | On reconnect report `run_failed { codex_crash }`, cleanup                                                         | `failed`                   |
| Runner process crash           | On next start: scan history, mark any "running" rows as `failed { runner_crashed }`, cleanup subprocess if leaked | `failed`                   |
| Codex subprocess crash mid-run | Try `thread/resume` once on fresh app-server; else `run_failed { codex_crash }`                                   | `failed` or `running`      |
| Laptop sleep                   | TCP hangs → WS ping fails → treat as drop → reconnect + `thread/resume` on wake                                   | `reconnected`              |
| Codex 401                      | Suspend run; emit `run_awaiting_reauth`; TUI + web UI prompt for re-auth; resume on user fix                      | `awaiting_reauth`          |
| Workspace clone failure        | `run_failed { workspace_setup, detail }`                                                                          | `failed`                   |
| `cancel` received              | `turn/interrupt`, wait 10s, SIGKILL app-server, cleanup, `run_cancelled`                                          | `cancelled`                |
| Cloud marks runner offline     | No local effect; runner keeps trying to reconnect                                                                 | —                          |

---

## 11. Code structure (Rust crate)

Single binary, multiple modules. Organized by concern:

```
pidash/
├── Cargo.toml
├── src/
│   ├── main.rs                     # CLI dispatch (clap)
│   ├── cli/
│   │   ├── mod.rs
│   │   ├── configure.rs            # `runner configure`
│   │   ├── service.rs              # `runner service [install|start|stop|status]`
│   │   ├── start.rs                # `runner start` (foreground)
│   │   ├── status.rs               # `runner status` (prints summary via IPC)
│   │   ├── tui.rs                  # `runner tui` (spawns Ratatui app)
│   │   ├── doctor.rs               # `runner doctor`
│   │   └── remove.rs               # `runner remove`
│   ├── daemon/
│   │   ├── mod.rs                  # main async orchestrator, state machine
│   │   ├── state.rs                # Idle / Busy / Reconnecting; current run
│   │   └── supervisor.rs           # task spawning + join handles
│   ├── cloud/
│   │   ├── mod.rs
│   │   ├── ws.rs                   # tokio-tungstenite client w/ reconnect + heartbeat
│   │   ├── protocol.rs             # serde enums: ClientMsg, ServerMsg
│   │   └── register.rs             # one-time token → credentials exchange (reqwest)
│   ├── codex/
│   │   ├── mod.rs
│   │   ├── app_server.rs           # spawn, stdio split, line framing
│   │   ├── jsonrpc.rs              # Request/Response/Notification types
│   │   ├── schema.rs               # enums for item/*, turn/*, account/*
│   │   └── bridge.rs               # cloud msg ↔ codex jsonrpc translation
│   ├── workspace/
│   │   ├── mod.rs
│   │   ├── resolve.rs              # working_dir logic (section 5)
│   │   └── git.rs                  # thin wrapper around system `git` command
│   ├── config/
│   │   ├── mod.rs
│   │   ├── file.rs                 # TOML read/write, 0600 perms
│   │   └── schema.rs               # Config, Credentials, ApprovalPolicy structs
│   ├── ipc/
│   │   ├── mod.rs                  # Unix socket server (daemon) + client (TUI/CLI)
│   │   ├── protocol.rs             # local JSON-RPC methods (see tui-design.md §"Local IPC")
│   │   └── server.rs
│   ├── history/
│   │   ├── mod.rs
│   │   ├── jsonl.rs                # append-only writer + reader
│   │   └── index.rs                # lightweight runs_index.json
│   ├── approval/
│   │   ├── mod.rs
│   │   ├── policy.rs               # evaluate() per section 9.1
│   │   └── router.rs               # fan-out to TUI + cloud; first-wins
│   ├── service/
│   │   ├── mod.rs
│   │   ├── systemd.rs              # generate user unit
│   │   └── launchd.rs              # generate plist
│   ├── tui/
│   │   ├── mod.rs                  # Ratatui app entry
│   │   ├── app.rs                  # model/update/view (Elm-ish)
│   │   ├── ipc_client.rs           # subscribes to daemon over Unix socket
│   │   └── views/
│   │       ├── status.rs
│   │       ├── config.rs
│   │       ├── approvals.rs
│   │       └── runs.rs
│   └── util/
│       ├── mod.rs
│       ├── paths.rs                # directories crate wrappers
│       ├── backoff.rs              # jittered exponential
│       └── signal.rs               # ctrl-c, sigterm handling
├── tests/
│   ├── cloud_protocol.rs           # round-trip serde tests
│   ├── codex_bridge.rs             # fake app-server → bridge → fake cloud
│   ├── workspace_resolve.rs
│   └── approval_policy.rs
└── dist/
    ├── Cargo.dist.toml             # cargo-dist config
    └── Formula/runner.rb           # generated Homebrew tap
```

### 11.1. Dependencies (MVP)

- `tokio` (full) — async runtime
- `tokio-tungstenite` + `rustls` — WS client
- `tokio-util` — codec utilities
- `serde` + `serde_json` + `toml` — data
- `ratatui` + `crossterm` — TUI
- `clap` (derive) — CLI
- `tracing` + `tracing-subscriber` + `tracing-appender` — logs
- `directories` — XDG + macOS paths
- `reqwest` (rustls, no cookies) — registration HTTP
- `thiserror` + `anyhow` — errors
- `uuid` — run IDs
- `self_replace` — self-update (post-v1)
- `rustls-pemfile` — TLS trust store loading
- `nix` (unix only) — file mode, signals

No C dependencies in MVP (rustls everywhere, no OpenSSL). Makes cross-compilation clean.

### 11.2. Expected LOC

~2,500–3,500 LOC at MVP maturity. Daemon/cloud + codex bridge are the largest modules (~1,000 each); TUI is ~600–800; everything else is small.

---

## 12. Distribution & releases

Tooling: **cargo-dist**.

- `cargo dist init` → `dist/Cargo.dist.toml` describing the targets: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`.
- Each tagged release: GitHub Actions matrix builds binaries + generates Homebrew formula, `.deb`, `.tar.gz`, SHA256SUMS.
- Homebrew tap hosted at `github.com/pi-dash/homebrew-tap`.
- `curl | sh` installer script resolves to the right target triple and drops binary in `$HOME/.local/bin`.

Binary signing:

- macOS: Developer ID codesign + notarize in CI. Without this, Gatekeeper will block first run.
- Linux: no signing required; users trust the GitHub release + checksum.

Self-update (v1.1+):

- Runner checks `/runner/version` on cloud once per day.
- If newer version advertised, download, verify checksum, replace binary with `self_replace`, re-exec.
- Respect a `config.auto_update = false` override.

---

## 13. Testing strategy

### 13.1. Unit tests

- `cloud::protocol` — round-trip serde for every message variant.
- `codex::schema` — deserialize real Codex event samples (checked in under `tests/fixtures/codex/`).
- `approval::policy::evaluate` — table-driven: each rule with positive and negative cases.
- `workspace::resolve` — tmpdir-based tests for empty/dir/repo/nonempty-nonrepo paths.

### 13.2. Integration tests

- **Fake cloud**: a small `tokio`-based WS server that drives the runner through assign → cancel / assign → approval → complete / WS-drop-resume scripts.
- **Fake Codex app-server**: a binary (compiled in `tests/bin/`) that reads stdin JSON-RPC and replays canned event streams. Runner's bridge talks to it exactly like real Codex.
- Combine: fake cloud + runner + fake codex → assert full scenarios end-to-end with no real network or real Codex.

### 13.3. TUI tests

- `ratatui`'s test backend captures rendered buffer; snapshot comparisons for each view in each state.

### 13.4. Manual QA matrix per release

- macOS arm64 (Sonoma+), macOS x64, Linux x64 (Ubuntu LTS + Fedora latest).
- Scenarios: first-run configure, service install/start/stop, TUI connect + each view, approval from TUI + approval from web, WS drop + reconnect, Codex auth expiry simulation.

---

## 14. Phasing

Aligns with the runner-side phases in `github-runner-architecture.md` §13.

### v1.0 — MVP

- [ ] CLI skeleton: `configure`, `start`, `service install/start/stop/status`, `status`, `doctor`, `remove`.
- [ ] Config + credentials file handling with proper perms.
- [ ] Cloud WS client with reconnect + heartbeat.
- [ ] Codex app-server stdio bridge.
- [ ] Workspace resolve + `git clone` on first task.
- [ ] Approval policy + routing (auto-approve allowlist + ask).
- [ ] Local JSONL history.
- [ ] TUI v1.0 views (Status, Config, Approvals, onboarding wizard) — see `tui-design.md`.
- [ ] Cargo-dist release pipeline for macOS arm64/x64 + Linux x64.
- [ ] Homebrew tap.

### v1.1

- [ ] Runs history view in TUI; doctor subcommand; search/filter.
- [ ] Linux arm64 build target.
- [ ] Self-update.

### v1.2

- [ ] Diff preview + log follower in TUI.
- [ ] Optional WebSocket transport to Codex app-server (vs stdio).

### v2

- [ ] Windows support (named pipe IPC, Windows Service, PowerShell integration).
- [ ] Optional event streaming to cloud (behind user opt-in).
- [ ] Native tray client reusing the local IPC.

---

## 15. Open questions / deferred

- **Multi-runner per machine.** Explicitly deferred. Global runner ID makes this a forward-compatible change (just add a "discriminator" label later).
- **Codex sandbox mode.** Deferred. MVP uses Codex's default `workspace-write` at `thread/start` time; no further enforcement.
- **Streaming Codex events to cloud.** Deferred. History is local-only; cloud can later pull history via a signed API if the user opts in.
- **Workflow templates + prompt composition.** Fully cloud-side; this doc deliberately ignores it beyond "prompt arrives in the assign message."
- **Observability/telemetry.** Minimal in MVP: only the WS heartbeat + local logs. No analytics, no crash reporting service. Revisit when paid users exist.

---

## 16. What Symphony-shaped logic _does not_ live here

For the reviewer's mental model, the runner **does not**:

- Poll any tracker.
- Maintain a scheduler or work queue.
- Compose prompts from templates.
- Decide retries.
- Interpret "done" semantics.
- Match tasks to runners.
- Store global run state (only its own local history).
- Open pull requests or run tests.

All of that lives in Pi Dash cloud. The runner is a mouthpiece for Codex with a persistent outbound connection and good manners about approvals and cleanup.
