# 07 — Runner Architecture

The runner is the agent on the developer's machine: a single Rust binary (`pidash`) that authenticates to Pi Dash cloud, pulls assigned runs, and drives an AI agent (`codex` or `claude`) as a subprocess.

It lives at `runner/` and is an **independent Cargo crate** — not part of the Turborepo workspace. Edition 2024, MSRV 1.93 (`rust-toolchain.toml` pins the toolchain).

## Module layout

```
runner/src/
├── main.rs               ← tokio entrypoint
├── lib.rs                ← module root
├── cli/                  ← clap subcommands
├── daemon/               ← supervisor + state machine (mod, observability, runner_instance, runner_out, state, supervisor)
├── cloud/                ← WS client + HTTP + message schemas (http, mod, projects, protocol, runners, ws)
├── codex/                ← `codex app-server` subprocess + JSON-RPC bridge (app_server, bridge, jsonrpc, mod, schema)
├── claude_code/          ← Claude Code adapter
├── agent/                ← generic agent trait (so new agents can be wired in)
├── approval/             ← policy engine + first-writer-wins router
├── workspace/            ← per-project working dir + `git clone` on first task
├── ipc/                  ← Unix socket / Windows named pipe between daemon and TUI/CLI
├── history/              ← JSONL per-run transcripts + recent-runs index
├── service/              ← systemd / launchd / Windows scheduled-task installers
├── tui/                  ← Ratatui UI (Status / Runs / Config / Approvals views)
├── config/               ← TOML config + credential files (0600 on Unix)
├── api_client.rs         ← HTTP API client (registration, status, version checks)
└── util/                 ← paths, logging, backoff, signal handling
```

## CLI surface (`cli/`)

The `pidash` binary is a clap subcommand router:

```
pidash auth login / logout / status      ← device-code login, token mgmt
pidash runner add / list / remove        ← register a runner on this host
pidash connect                            ← legacy enrollment-token flow
pidash configure                          ← (re)write config
pidash install / uninstall                ← install OS service unit
pidash start / stop / restart             ← daemon lifecycle
pidash status                             ← service + daemon status
pidash tui                                ← interactive TUI
pidash doctor                             ← preflight checks (agent on PATH, cloud reachable)
pidash update [--check|--restart]         ← self-update via cargo-dist receipt
pidash issue / comment / state / workspace ← assorted helper commands
pidash __run                              ← hidden internal run wrapper
```

Bare `pidash` (no subcommand) drops into `auth login` when no config exists — useful as the first-run path after MSI install.

## Daemon (`daemon/`)

The daemon is the long-lived process. Its responsibilities:

1. **Supervisor** (`supervisor.rs`) — drives the state machine: connect → register → poll for runs → dispatch → report.
2. **Runner instance** (`runner_instance.rs`) — one daemon can manage multiple runner registrations (one per project on this host).
3. **Observability** (`observability.rs`) — structured logging + metrics.
4. **State** (`state.rs`) — in-memory state shared between IPC, TUI, and supervisor.

The daemon talks to the cloud via `cloud/` (HTTPS + WS) and to the local TUI via `ipc/` (Unix socket on Unix, named pipe on Windows — both `0600`).

## Codex / Claude Code bridges

Codex (`codex/`) is the first-class integration:

- `app_server.rs` spawns `codex app-server` as a subprocess.
- `jsonrpc.rs` is the JSON-RPC client side of the bridge.
- `bridge.rs` mediates between the cloud-side run state and the agent process.
- `schema.rs` types the JSON-RPC messages.

Claude Code (`claude_code/`) and the generic `agent/` trait are the abstraction that lets additional agents be wired in without re-doing the orchestration layer. Adding a new agent kind means implementing the trait and wiring it into the supervisor's dispatch.

## Approval router (`approval/`)

When the agent asks for approval (run command, write file, fetch URL — depends on agent policy), the runner has three potential decision sources:

1. The **TUI** (interactive operator on this host)
2. The **cloud** (web UI operator)
3. The local **policy engine** (rules in the runner's config)

Decisions race; **first writer wins**. The router enforces this. The policy engine can pre-approve common patterns to avoid bombing the operator with prompts.

## Workspace (`workspace/`)

A runner can be registered to a project. On the first run for that project, the runner `git clone`s the project's repo into a configured working dir. Subsequent runs reuse the clone (fetch + checkout, not re-clone).

This is also where path constraints live — the runner refuses to operate on paths outside the configured workspace root.

## IPC (`ipc/`)

- **Unix:** Unix domain socket under `$XDG_RUNTIME_DIR/...`, `0600`.
- **Windows:** local named pipe.

Used by:

- TUI ↔ daemon — push state updates, send approval decisions
- CLI subcommands ↔ daemon — `pidash status`, `pidash stop`, etc.

## TUI (`tui/`)

Built on Ratatui. Views:

- **Status** — connection state, version banner, agent path
- **Runs** — recent runs + their phase + transcripts
- **Config** — editable daemon settings (auto-update toggle lives here)
- **Approvals** — pending prompts, decision UI

## On-disk layout (Linux example)

```
~/.config/pidash/             ← config.toml, credentials (0600)
$XDG_DATA_HOME/pidash/        ← per-run JSONL transcripts, recent-runs index
$XDG_RUNTIME_DIR/pidash/      ← IPC socket, PID file
```

On Windows, everything goes under the user profile and IPC is via a named pipe.

## Auto-update

The cloud's `welcome` frame can include `latest_runner_version` and `min_runner_version` (driven by the cloud's `LATEST_RUNNER_VERSION` / `MIN_RUNNER_VERSION` env vars). With auto-update enabled, the daemon swaps the on-disk `pidash` binary in place; the **running process is never disturbed** — it keeps its loaded copy until the next natural restart (`pidash restart`, host reboot, service-manager respawn).

`pidash update` only works for installs from cargo-dist installers (`pidash-installer.sh`, `pidash-installer.ps1`, MSI) — those leave an install receipt the updater reads. Source builds and `cargo install` builds get a clear "reinstall via the installer if you want self-update" error. See [15 — Releasing](./15-releasing.md).

## Where to read next

- [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md) — the wire schema for `cloud/`
- [11 — Authentication](./11-authentication.md) — device-code login + token rotation
- `runner/README.md` — install one-liners, paths, manual QA matrix
- `runner/.ai_design/implement_runner/` (if present) — design docs and committed decisions
