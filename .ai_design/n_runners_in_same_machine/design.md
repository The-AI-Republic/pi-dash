# Multi-Runner Daemon — Design

> **Note (2026-04-29):** the workspace-default-pod assumption originally baked
> into §4–§6 has been refactored into a project-scoped pod model. Each project
> now owns at least one pod (the auto-created default); a runner is bound to
> exactly one project via its pod. The token + WebSocket multiplex layer below
> is unchanged. See
> [`new_pod_project_relationship/`](./new_pod_project_relationship/) for the
> updated data model, schema migration (`runner/migrations/0007_*`), runner
> registration body shape (`project` / `pod` fields), and CLI surface
> (`pidash configure --project`, `pidash token add-runner --project`,
> `pidash token list-projects`).

A single `pidash` daemon process per dev machine hosts **N independently-registered runner instances** that share a single multiplexed WebSocket session to Pi Dash cloud. From the cloud's perspective every runner instance is fully independent: its own `runner_id`, its own assignments, its own status, its own history. The shared transport is a transport-layer optimisation that the cloud's business logic never observes.

This document supersedes the single-runner-per-process assumption baked into `runner-design.md`.

---

## 1. Goals

- One OS process per machine, regardless of how many runners are configured.
- Multiple logical runners per process, each appearing to the cloud exactly as today's single runner does.
- Each instance remains **single-tenant** (≤ 1 in-flight run). Concurrency comes from running multiple instances, not parallelising inside one.
- One shared WebSocket session multiplexed across all instances.
- Runners can be added or removed at runtime without restarting the daemon.

## 2. Non-goals

- Side-by-side daemon processes on the same machine (explicitly out of scope — see `runner_install_ux/`).
- Concurrent runs within one runner instance (separate problem; would require workspace isolation work).
- N WebSocket sessions from one process (considered and rejected; ADR §15).
- A daemon that hosts runners across multiple Pi Dash workspaces. All runners on one daemon belong to a single Pi Dash workspace (the one the token was created in). Cross-workspace machine identity is deferred — see Q7 in `decisions.md`.

## 3. Vocabulary

| Term                                  | Meaning                                                                                                                                                                                                                                      |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Daemon**                            | The OS process. Owns the IPC socket, the cloud transport, signal handling, logging, the supervisor task. Cardinality 1 per machine.                                                                                                          |
| **Runner instance**                   | A logical runner with its own `runner_id`, agent config, working directory, approval policy, history dir, and single in-flight slot. Runners do not own a credential — they're identifiers under a token (see §5). Cardinality N per daemon. |
| **Token** (a.k.a. machine credential) | A cloud-side entity that authenticates the WS connection and is authorised to act as a declared set of `runner_id`s. Surfaced in the UI as a "token" with a user-supplied title. See §5.                                                     |
| **Shared connection**                 | The single WebSocket session opened by the daemon, multiplexed across all instances.                                                                                                                                                         |

## 4. Wire protocol v2

### 4.1 Envelope change

In a single-runner-per-connection model, every frame is implicitly bound to "the runner this WS belongs to" — auth headers pin identity at HTTP-upgrade time and frames carry no `runner_id` discriminator. To multiplex N runners across one WS, every routed frame must name its target.

Add `runner_id` to `Envelope` as an optional top-level field:

```rust
pub struct Envelope<T> {
    pub version: u32,                // bump to 2
    pub message_id: Uuid,
    pub runner_id: Option<Uuid>,     // None = connection-scoped frame
    pub body: T,
}
```

Bump `WIRE_VERSION = 2`. Cloud accepts both v1 and v2 during migration:

- **v1 frame** → cloud infers `runner_id` from the connection's authenticated identity (must be exactly one).
- **v2 frame** → cloud trusts `Envelope.runner_id`; rejects frames whose id isn't in the connection's authenticated set.

### 4.2 Routing rules

| Frame                                                                                               | `Envelope.runner_id` | Notes                                                                                                                                                                                                            |
| --------------------------------------------------------------------------------------------------- | -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Hello`                                                                                             | `Some(id)`           | Identifies the runner being authorised. Body still carries `runner_id` to keep `Hello` self-contained.                                                                                                           |
| `Welcome`                                                                                           | `Some(id)`           | One `Welcome` per `Hello`. Acks a specific runner, not the connection.                                                                                                                                           |
| `Heartbeat`                                                                                         | `Some(id)`           | One heartbeat envelope per instance per tick.                                                                                                                                                                    |
| `Accept` / `RunStarted` / `RunEvent` / `RunCompleted` / `RunFailed` / `RunCancelled` / `RunResumed` | `Some(id)`           | Per-runner.                                                                                                                                                                                                      |
| `ApprovalRequest` / `Decide`                                                                        | `Some(id)`           | Per-runner.                                                                                                                                                                                                      |
| `Cancel`                                                                                            | `Some(id)`           | Cloud already targets a `run_id`; runner_id picks the destination instance.                                                                                                                                      |
| `RunAwaitingReauth`                                                                                 | `Some(id)`           | Per-runner; no longer flips a global daemon status.                                                                                                                                                              |
| `ConfigPush`                                                                                        | `Some(id)`           | Per-runner config push only at this stage — pushes `approval_policy` to one runner. The protocol shape allows `None` for a daemon-wide push, but no daemon-level field is currently remotely-pushable. See §9.2. |
| `Ping` / response                                                                                   | `None`               | Connection-scoped keepalive.                                                                                                                                                                                     |
| `Bye`                                                                                               | `None`               | Connection teardown.                                                                                                                                                                                             |
| `Revoke`                                                                                            | `None`               | The token was revoked. Connection is torn down; daemon shuts down all instances. There is no per-runner Revoke — the cloud-side UI does not surface revocation per runner (see Q4 in `decisions.md`).            |
| `RemoveRunner`                                                                                      | `Some(id)`           | Cloud-initiated decommission of one runner. Daemon cancels its in-flight run, removes the instance, drops its mailbox, and frees its working directory. Connection and other instances stay up.                  |

### 4.3 Demux rule (authoritative)

There is exactly one rule for the demux task — every frame goes to one of two consumers, never both:

- **Connection consumer** (the supervisor's connection task): consumes any frame with `Envelope.runner_id = None`. That set is exactly `{ Ping, Bye, Revoke }`.
- **Instance mailbox**: consumes any frame with `Envelope.runner_id = Some(id)`. That set is everything else, including `Welcome`. The frame is delivered to `instances[id]`'s mailbox; if no such instance exists, the demux logs a warning and drops.

The `Welcome` frame deserves a note: it is **per-runner**. There is no connection-level Welcome. Cloud sends one `Welcome { runner_id }` per `Hello`; the protocol/heartbeat parameters in the `Welcome` body are connection-level values but are read by each instance's mailbox-side handler and propagated to the connection state via a watch channel. The supervisor accepts whichever `Welcome` arrives first as authoritative for those connection-level fields and ignores subsequent values (they should agree).

`Ping` is handled exclusively by the connection task: one `Ping` from cloud → one `Heartbeat` reply with `Envelope.runner_id = None` and an empty per-runner status (`status: Idle`, `in_flight_run: None` at the connection level — the per-runner heartbeats already carry the real per-instance status, see §6.6). This is a wire change from v1, where `Ping` was handled inside `RunnerLoop` (`runner/src/daemon/supervisor.rs:327`); in v2 it moves out of the inner loop.

## 5. Auth model

The shared WS authenticates as a **token** (a machine credential), not as any single runner. Each runner has a stable `runner_id` (used as a routing identifier on the wire and a display key in the UI) but no per-runner secret.

### 5.1 Token (machine credential)

New cloud entity:

```
Token {
    token_id: Uuid,
    title: String,                // user-supplied label, shown in the UI
    secret: <hashed>,             // shown to the user once, at creation
    workspace_id: Uuid,           // tokens are scoped to one Pi Dash workspace
    owns: Vec<Uuid>,              // runner_ids this token may act as
    created_at, last_seen_at, revoked_at: Option<DateTime>,
}
```

Mental model: **one dev machine == one daemon == one token == one WS connection == N runner instances.** The token is what authenticates the connection; runner_ids are routing keys.

### 5.2 Lifecycle

- **Token creation**: user creates a token in the Pi Dash web UI; cloud returns `token_id` + `secret` (shown once). User pastes them into `pidash configure token` on the dev machine.
- **Runner registration**: `pidash configure runner --name <name>` calls the cloud, which mints a `runner_id`, attaches it to the active token's `owns` set, and persists locally in `credentials.toml`.
- **Connection auth**: WS upgrade headers carry `X-Token-Id` + `Bearer <token_secret>`. Cloud loads the token, verifies the secret, populates the connection's authorised set with `Token.owns`.
- **Hello exchange**: after WS upgrade, daemon sends `Hello { runner_id }` for each instance it wants to bring online. Cloud verifies each `runner_id ∈ Token.owns` and emits `Welcome { runner_id }`. A `Hello` for an unowned runner gets a `RemoveRunner` reply (see §4.2 / §11).
- **Token revocation**: cloud-initiated, only path for retiring a token. Cloud sends connection-scoped `Revoke`; current connection auth fails on next reconnect. Daemon shuts down. All runners owned by the token go offline together. **In-flight runs are hard-cancelled** (no graceful "commit WIP" attempt); the agent subprocess gets the standard 5s grace then SIGKILL.
- **Runner removal**: cloud-initiated `RemoveRunner { runner_id }` (or local `pidash remove --runner <name>`). Removes the runner from `Token.owns` cloud-side and from `instances` daemon-side. **Daemon also deletes the runner's local data** (`data_dir/runners/<runner_id>/` — history, logs, identity). Once a runner is removed, its data is gone. Token and other runners unaffected.

**Tokens are not rotatable.** The supported lifecycle is create + revoke. To "rotate" a token, the user creates a new one in the UI, runs `pidash configure token` to install it, then revokes the old one in the UI. There is no in-place rotation API; the design is deliberately simpler at the cost of a brief overlap window when both tokens exist.

### 5.2.1 Credential scope: two surfaces, two credentials

The runner has two distinct auth surfaces with separate credentials:

| Surface                    | Credential     | Header                               |
| -------------------------- | -------------- | ------------------------------------ |
| WS upgrade (`/ws/runner/`) | `token_secret` | `X-Token-Id` + `Bearer token_secret` |
| REST (`/api/v1/...`)       | `api_token`    | `X-Api-Key: <api_token>`             |

`Credentials` carries:

- `token_id` + `token_secret` — for WS auth.
- `api_token` — for REST auth (orthogonal to the WS auth model; already token-based).
- `runner_id` per runner — identifier only, not a credential.

**Why not unify WS and REST onto one token**: it's a separate auth-system redesign that would touch every `/api/v1/` endpoint, the `PIDASH_TOKEN` env path (`runner/src/api_client.rs:8`), the cloud's API key middleware, and every contract test on the v1 surface. Out of scope here; a follow-up can unify both surfaces if it's wanted.

When a CLI verb addresses a specific runner (e.g. `pidash issue --runner main`), the runner_id is a request parameter on the REST surface; the `api_token` authenticates the call.

### 5.3 UI surface

The Pi Dash cloud UI separates "tokens" and "runners":

- **Tokens section**: lists active tokens, each with its title. Each token entry expands to show the runners associated with it. Token has a **Revoke** action (kills the token, all its runners go offline).
- **Runners section**: lists runners (under their owning tokens). Each runner has a **Remove** action (decommissions that one runner). **No Revoke action on runners** — revocation is a token-level concept only.

This matches the "credential is a security primitive; runner is an operational primitive" split: you revoke a credential when it leaks; you remove a runner when it's no longer needed.

### 5.4 Why not per-runner secrets

Considered and rejected: a per-runner `runner_secret` model where the WS authenticates as one runner via headers and additional runners send `Hello` frames in-band carrying their own secrets. Rejected because:

- Bootstrap-vs-rest auth asymmetry is awkward (the connection's identity is one specific runner, but it carries traffic for many).
- Rotating the bootstrap runner's secret would tear down the connection.
- Audit logs name a runner for traffic that has nothing to do with that runner.
- Per-runner secrets multiply credential management overhead with no upside given that the cloud already needs to know which runners are owned by which host for billing/quotas.

## 6. Daemon-internal architecture

```
                     ┌──────────────────────────────────────────────────────┐
                     │  Supervisor (cardinality 1)                          │
                     │                                                      │
                     │   instances: HashMap<Uuid, Arc<RunnerInstance>>      │
                     │                                                      │
                     │   ┌──────────────┐   ┌─────────┐   ┌────────────┐    │
   inbound (WS)  ──► │   │ ConnectionLp │──►│  Demux  │──►│ mailbox A  │ ──►│ RunnerLoop A
                     │   │ (cloud/ws.rs)│   │  task   │──►│ mailbox B  │ ──►│ RunnerLoop B
                     │   └──────────────┘   └─────────┘──►│ mailbox C  │ ──►│ RunnerLoop C
                     │                          ▲                           │
                     │                          └─ supervisor inbox         │
                     │                             (Ping, Bye,               │
                     │                              connection Revoke)       │
                     │                                                      │
   outbound (WS) ◄── │   shared out_tx ◄────────── Mux (just an mpsc) ◄─── RunnerOut(A)
                     │                                                  ◄── RunnerOut(B)
                     │                                                  ◄── RunnerOut(C)
                     │                                                      │
                     │   Heartbeat task ── for each instance, send          │
                     │                     Envelope{runner_id: id, body:    │
                     │                              Heartbeat{...}}        │
                     │                                                      │
                     │   IpcServer ──── client commands carry               │
                     │                  `runner: <name|id>`                 │
                     └──────────────────────────────────────────────────────┘
```

### 6.1 Supervisor

`Supervisor` (replaces the current struct in `runner/src/daemon/supervisor.rs:21`):

```rust
pub struct Supervisor {
    pub config: DaemonConfig,
    pub token_creds: TokenCredentials,            // see §5.1; one per daemon
    pub paths: Paths,
    pub opts: Options,
    pub instances: Arc<RwLock<HashMap<Uuid, Arc<RunnerInstance>>>>,
    pub connection_state: ConnectionStateHandle,  // shared; per-connection, not per-runner
    pub out: mpsc::Sender<Envelope<ClientMsg>>,   // shared by all instances
}
```

The supervisor's `run()` spawns:

1. The `IpcServer` (one).
2. The `ConnectionLoop` (one) which owns the WS and produces a `mpsc::Receiver<Envelope<ServerMsg>>`.
3. The `Demux` task that consumes inbound frames and routes to per-instance mailboxes or the supervisor inbox.
4. The heartbeat task that iterates `instances` and emits one heartbeat envelope per instance.
5. One `RunnerLoop` per instance, each with its own mailbox.

When `pidash configure --instance` adds a new instance at runtime, the supervisor inserts into `instances`, sends `Hello { runner_id }` over the existing WS, and spawns a new `RunnerLoop`. No reconnect.

### 6.2 RunnerInstance

```rust
pub struct RunnerInstance {
    pub runner_id: Uuid,
    pub name: String,
    pub config: RunnerConfig,         // agent, workspace, approval_policy
    pub state: RunnerStateHandle,     // per-instance status, current_run, approvals_pending
    pub approvals: ApprovalRouter,    // per-instance
    pub paths: RunnerPaths,           // per-instance history/, logs/
    pub mailbox_tx: mpsc::Sender<Envelope<ServerMsg>>,
    pub out: RunnerOut,               // newtype wrapping shared out_tx with this runner_id
}
```

`RunnerLoop` (the existing inner loop in `supervisor.rs:204`) is reshaped slightly. It receives `Envelope<ServerMsg>` from its mailbox and dispatches `Welcome` / `Assign` / `Cancel` / `Decide` / `ConfigPush` / `ResumeAck` / `RemoveRunner`. **It no longer handles `Ping` or `Revoke`** — those are connection-scoped frames consumed by the supervisor's connection task (see §4.3). The single `current_run: Option<CurrentRun>` field stays exactly as today.

### 6.3 RunnerOut

```rust
struct RunnerOut {
    runner_id: Uuid,
    inner: mpsc::Sender<Envelope<ClientMsg>>,
}

impl RunnerOut {
    async fn send(&self, body: ClientMsg) {
        let mut env = Envelope::new(body);
        env.runner_id = Some(self.runner_id);
        let _ = self.inner.send(env).await;
    }
}
```

Every existing `worker.send(ClientMsg::…)` and `out.send(Envelope::new(…))` site (e.g. supervisor.rs:286-294, 446-451, 480-484) becomes `out.send(ClientMsg::…)` against the `RunnerOut` newtype. The runner_id is set in exactly one place; no caller can forget.

### 6.4 Demux

```rust
async fn demux(
    mut rx: mpsc::Receiver<Envelope<ServerMsg>>,
    instances: Arc<RwLock<HashMap<Uuid, Arc<RunnerInstance>>>>,
    supervisor_tx: mpsc::Sender<Envelope<ServerMsg>>,
) {
    while let Some(env) = rx.recv().await {
        match (env.runner_id, &env.body) {
            (None, _) => { let _ = supervisor_tx.send(env).await; }
            (Some(id), _) => {
                let map = instances.read().await;
                if let Some(inst) = map.get(&id) {
                    let _ = inst.mailbox_tx.send(env).await;
                } else {
                    tracing::warn!(%id, "frame for unknown runner; dropping");
                }
            }
        }
    }
}
```

Frames for unknown runner_ids are dropped with a warning rather than tearing down the connection — defensive in case the cloud sends for a stale runner during a reconfigure window.

### 6.5 Connection state machine

`ConnectionStateHandle` tracks:

- `connected: bool` (one TCP+WS, one bool).
- `last_heartbeat_ack: Option<DateTime<Utc>>`.
- `authorised_runners: HashSet<Uuid>` — populated as `Welcome { runner_id }` frames arrive (each `Welcome` goes to the matching instance's mailbox, and the instance's handler also notifies the connection state via a watch channel), cleared on disconnect.

Per-runner status (`RunnerStatus::Idle/Busy/Reconnecting/AwaitingReauth`) lives on each instance's `RunnerStateHandle`, separate from connection state.

### 6.6 Heartbeats

Two distinct streams, both running over the shared connection:

- **Per-runner heartbeats** (one per instance, every `heartbeat_interval_secs`): each instance's heartbeat task emits `Envelope { runner_id: Some(id), body: Heartbeat { ts, status, in_flight_run } }` carrying the instance's real status. This is the cloud's authoritative liveness signal _per runner_.
- **Connection-level pong** (response to cloud's `Ping`): the connection task replies once per `Ping` with `Envelope { runner_id: None, body: Heartbeat { ts, status: Idle, in_flight_run: None } }`. The status fields are carried as zero-values; the cloud treats per-runner heartbeats as the source of truth for per-runner state, and uses the connection-level pong only as a transport-liveness probe.

Cloud-side: per-runner heartbeats update `runners.last_seen_at`; connection-level pongs update `tokens.last_seen_at`.

## 7. Per-instance state fan-out

| Today (singleton)                                              | Multi-runner shape                                                                                               |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `Credentials`                                                  | `TokenCredentials` (one per daemon) + `Vec<RunnerIdentity>` (N; just `runner_id` + `name`, no per-runner secret) |
| `Config.agent` / `Config.workspace` / `Config.approval_policy` | Fields on `RunnerConfig`, one per instance                                                                       |
| `StateHandle.current_run: Mutex<Option<CurrentRunSummary>>`    | Per-instance                                                                                                     |
| `StateHandle.tx_in_flight: watch::Sender<Option<Uuid>>`        | Per-instance                                                                                                     |
| `StateHandle.approvals_pending`                                | Per-instance                                                                                                     |
| `StateHandle.runner_id`                                        | Per-instance                                                                                                     |
| `ApprovalRouter`                                               | Per-instance                                                                                                     |
| `HistoryWriter` paths (`data_dir/history/runs/`)               | `data_dir/runners/<runner_id>/history/runs/`                                                                     |
| `RunsIndex`                                                    | Per instance                                                                                                     |
| `RunnerLoop.current_run: Option<CurrentRun>`                   | Per instance (each is single-tenant — unchanged shape)                                                           |

Stays daemon-singleton: IPC socket, PID file, the WS connection, logging, signal watcher, the systemd/launchd unit, `runtime_dir`.

## 8. Persistence layout

```
data_dir/
  runtime/
    pidash.sock
    pid
  token/
    token_id                 (or kept in credentials.toml)
    token_secret             (or kept in credentials.toml; mode 0600)
    title                    (cached locally for display in `pidash status`)
  runners/
    <runner_id_1>/
      identity.toml          (runner_id, name, registered_at, workspace_slug)
      history/
        runs/
        runs_index.json
      logs/
    <runner_id_2>/
      ...
```

Per-runner directories never need to be merged. Logs segregated by runner make incident triage cheaper.

`Paths::resolve` (`runner/src/util/paths.rs`) gains `runner_dir(runner_id) -> PathBuf` and the existing `history_dir() / runs_dir() / runs_index_path() / logs_dir()` accept a `runner_id` argument or are replaced with per-instance `RunnerPaths` carrying a baked-in id.

## 9. Config shape

```toml
# config.toml

[daemon]
cloud_url = "https://cloud.pidash.so"
heartbeat_interval_secs = 25
log_level = "info"

[[runner]]
name = "main"
runner_id = "..."              # set by `pidash configure --instance main`
[runner.workspace]
working_dir = "/home/rich/work/main"
[runner.agent]
kind = "codex"
[runner.approval_policy]
auto_allow = ["read"]

[[runner]]
name = "side-project"
runner_id = "..."
[runner.workspace]
working_dir = "/home/rich/work/side"
[runner.agent]
kind = "claude_code"
```

`credentials.toml` carries:

```toml
[token]
token_id = "..."
token_secret = "..."           # mode 0600
title = "rich's laptop"        # cached locally for display

[[runner]]
runner_id = "..."
name = "main"

[[runner]]
runner_id = "..."
name = "side-project"
```

No per-runner secret. The token authenticates the connection; `runner_id`s are routing keys.

**Validation at daemon startup** (all hard errors — daemon refuses to start with a detailed message):

- **Duplicate working_dir** — two instances must never share a working directory. Concurrent `git checkout` / file writes against the same `.git/` corrupt state silently. Refuse to start with:
  ```
  configuration error: runners "main" and "side-project" share working_dir "/home/rich/work".
  Each runner must have its own working directory. Update one of them in config.toml.
  ```
- **Nested working_dirs** — also refused (one path is a strict prefix of another). Same family of corruption hazard. Detailed error names both runners and both paths.
- **Duplicate `runner_id`** — refused. Cloud-side state is keyed by runner_id; collisions break routing.
- **Duplicate `name`** — refused. Names are user-facing; collisions break `--runner <name>` selection.
- **Instance count > cap (50)** — refused. See §16.
- **`credentials.toml` exists but has no `[token]` block** — refused. See §13.3 for the message and recovery path.
- Zero instances configured — _not_ an error. Daemon comes up idle and IPC-only, useful for `pidash configure runner` to add the first instance.

### 9.2 Config scopes and ConfigPush

Two scopes:

| Scope                           | Fields                                              | Source of truth                                                                                                               |
| ------------------------------- | --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| **Daemon-level** (one slice)    | `cloud_url`, `heartbeat_interval_secs`, `log_level` | Local `config.toml` `[daemon]` block; `heartbeat_interval_secs` is overridden by `Welcome` from the cloud at connection time. |
| **Runner-level** (per-instance) | `agent`, `workspace`, `approval_policy`             | Local `config.toml` `[[runner]]` block; `approval_policy` is overridable by cloud-pushed updates.                             |

Two runners can run different agents (codex vs claude_code), against different repos, with different approval policies. None of them share state.

**`ConfigPush` from cloud**: today carries `approval_policy` and is **per-runner only** — `Envelope.runner_id = Some(id)` selects which runner's policy to update. The protocol shape leaves room for a daemon-wide push (`runner_id = None`), but no daemon-level field is currently remotely pushable, so that path has no consumer. If a future field needs daemon-wide remote update, document the scope choice explicitly at that time.

## 10. IPC and TUI

### 10.1 User journeys

The runner has one user-facing creation entry point: an **Add Runner** action in the runners view of the cloud UI. It branches into "new connection" (= new machine setup) or "existing connection" (= additional runner on a machine the user already has set up). The CLI is also a valid entry point — invoking it directly bypasses the UI.

Vocabulary in this section:

- **Connection** = a `Token` row + the WS session it authenticates. One connection per dev machine.
- **Runner** = a `Runner` row + a `[[runner]]` block in local `config.toml`. N runners per connection.

#### 10.1.1 New connection + first runner (UI-driven)

```
Cloud UI                         Add Runner modal
─────────────────────────────────────────────────────────
                                 ○ New connection
                                 ◉ Existing connection ▾
                                 ─────────────────────────
                                 Connection title:  [_________]   (required)
                                 Runner name:       [_________]   (default: hostname)
                                 Working dir:       [_________]   (default: $TMPDIR/.pidash/)
                                 Agent:             [codex ▾]
                                 ▸ Approval policy (advanced)
                                 ─────────────────────────
                                 [Generate setup script]
```

User fills the form, clicks **Generate setup script**. Cloud:

1. Creates a `Token` row with the given title, mints `token_id` + `token_secret`.
2. Returns a setup script with all the values baked in:

   ```bash
   curl -fsSL https://get.pidash.so/install.sh | sh -s -- \
     --token-id    <token_id> \
     --token-secret <token_secret> \
     --connection-title "work laptop" \
     --runner-name "rich-laptop" \
     --working-dir "/tmp/.pidash" \
     --agent codex
   ```

User pastes the script on the dev machine. The script (non-interactive):

1. Installs `pidash` if not present.
2. Writes `[token]` block to `credentials.toml` (mode 0600).
3. Calls `POST /api/v1/runner/register/` (auth `X-Token-Id` + `Bearer token_secret`) to mint the first runner with the user's chosen name + working_dir + agent + approval_policy.
4. Writes the `[[runner]]` block to `config.toml`.
5. Installs the systemd/launchd unit (if not already installed).
6. Starts the daemon.

End state: one connection, one runner, daemon running.

#### 10.1.2 Additional runner on existing connection (UI-driven)

User opens the same Add Runner modal, picks **Existing connection**, selects a connection from a dropdown of their tokens (by title). Form fields are the same minus the connection title:

```
                                 ○ New connection
                                 ◉ Existing connection ▾  [work laptop ▾]
                                 ─────────────────────────
                                 Runner name:       [_________]   (default: hostname-2)
                                 Working dir:       [_________]
                                 Agent:             [codex ▾]
                                 ▸ Approval policy (advanced)
                                 ─────────────────────────
                                 [Generate command]
```

Cloud generates a CLI invocation (no token credentials needed — the target machine already has them):

```bash
pidash configure runner \
  --name "rich-laptop-2" \
  --working-dir "/home/rich/work/side" \
  --agent claude_code
```

User pastes it on the target machine. The CLI:

1. Calls `POST /api/v1/runner/register/` (auth from the existing `[token]` block) to mint the runner.
2. Writes the `[[runner]]` block to `config.toml`.
3. Tells the running daemon over IPC: "load instance `<runner_id>`."
4. Daemon inserts into `instances`, sends `Hello { runner_id }` over the existing WS, receives `Welcome`, spawns the `RunnerLoop`. No reconnect.

End state: existing connection, +1 runner, daemon kept running throughout.

#### 10.1.3 Additional runner via direct CLI (no UI)

The user can skip the cloud UI entirely and run `pidash configure runner` on the dev machine directly:

```bash
$ pidash configure runner --name side --working-dir ~/work/side --agent claude_code
```

Cloud-side and daemon-side effect is identical to §10.1.2 — the only difference is the user typed the command themselves instead of copying it from the UI. Useful for users who live in a terminal or are scripting setup.

If invoked on a TTY with required fields missing, the CLI prompts (matches today's behavior at `runner/src/cli/configure.rs:34-44`). On a non-TTY (e.g. cloud-init scripts), missing fields produce a clear error.

#### 10.1.4 Editing a runner

Three valid paths, all converging on the same daemon-side reload:

- **CLI flag (partial-edit):**

  ```bash
  $ pidash configure runner --name main --approval-auto-readonly true
  > Updated approval_policy.auto_approve_readonly_shell = true for "main".
  ```

  Only the flags you pass change. Daemon reloads that runner's config slice in place; no reconnect.

- **TUI:**

  ```bash
  $ pidash tui
  # Pick "main" from the runner picker → Config tab → edit → save.
  ```

  Best for list-valued fields like the approval allowlist that are awkward on the CLI.

- **Direct edit + reload:**
  ```bash
  $ $EDITOR ~/.config/pidash/config.toml
  $ pidash configure --reload      # tells daemon to re-read config.toml
  ```
  Escape hatch for power users.

Daemon-level fields (`cloud_url`, `heartbeat_interval_secs`, `log_level`) are not editable per-runner. Changing them via direct edit + reload requires a daemon restart for `cloud_url`; the other two can be hot-applied.

#### 10.1.5 Removing

Per-runner removal:

```bash
$ pidash remove --runner side
> Cancelling in-flight run for "side"... (none)
> Deregistering "side" cloud-side... OK.
> Removed [[runner]] block from config.toml.
> Deleted /home/rich/.local/share/pidash/runners/9c1b7e.../
> Daemon dropped the instance.
```

Full teardown (today's `pidash remove` behavior):

```bash
$ pidash remove
> Stopping daemon... OK.
> Uninstalling service unit... OK.
> Revoking token cloud-side (cascades to all runners under it)... OK.
> Deleted config.toml, credentials.toml, data_dir/runners/*.
```

The two are disambiguated by the `--runner` flag.

### 10.2 CLI verb reference

Every IPC verb that today implicitly addresses the runner gains a `--runner <name|id>` selector, with the rule **"if exactly one instance is configured, the flag is optional"**.

- `pidash status` → lists all instances; `pidash status --runner main` for one.
- `pidash configure` → splits into `pidash configure token` (one-shot per host; pastes in the token + secret created via the cloud UI) and `pidash configure runner --name <name>` (per instance, registers the runner under the active token).
- `pidash remove` → **two distinct verbs, disambiguated by flag**:
  - `pidash remove` (no flag): full machine teardown, today's behavior (`runner/src/cli/remove.rs`). Stops the service, uninstalls the unit, deregisters the _token_ from the cloud (which cascades to all owned runners), deletes `config.toml` + `credentials.toml` + `data_dir/runners/*`. This is the inverse of `install` + `configure` for the whole host.
  - `pidash remove --runner <name>`: per-instance removal. Cancels that runner's in-flight run, removes it from `[[runner]]` in `config.toml`, deletes `data_dir/runners/<runner_id>/`, and calls a REST endpoint (`POST /api/v1/runner/<runner_id>/deregister/` authenticated with the token) to tell cloud the runner is gone. **No WS frame is emitted** — `Bye` is reserved for connection teardown (§4.2). Future cloud frames for the deregistered `runner_id` (in unlikely race) get dropped by the demux as "unknown runner." Connection and other runners stay up.
- `pidash tui` → instance picker / multi-pane view.
- `pidash issue …` / `pidash comment …` etc. that talk to cloud need to know which runner identity to use; default to single instance, require `--runner` otherwise.
- Approvals over IPC carry `runner_id` (or `runner_name`).

### 10.3 IPC `StatusSnapshot`

`StatusSnapshot` (in `runner/src/ipc/protocol.rs:57`) becomes:

```rust
pub struct StatusSnapshot {
    pub daemon: DaemonInfo,                  // started_at, cloud_url, connected, last_heartbeat
    pub runners: Vec<RunnerStatusSnapshot>,
}

pub struct RunnerStatusSnapshot {
    pub runner_id: Uuid,
    pub name: String,
    pub status: RunnerStatus,
    pub current_run: Option<CurrentRunSummary>,
    pub approvals_pending: usize,
}
```

This is a breaking IPC change, but the IPC wire is private — bump the IPC version and update the TUI and CLI in the same release.

## 11. Connection lifecycle

### 11.1 Cold start

1. Daemon reads config → builds `RunnerInstance` objects (mailboxes, state handles, paths).
2. `ConnectionLoop` opens WS using `TokenCredentials` (`X-Token-Id` + `Bearer token_secret` headers).
3. On WS upgrade, supervisor walks `instances` and sends `Hello { runner_id }` for each.
4. Each instance stays in `Reconnecting` until its `Welcome { runner_id }` arrives, then flips to `Idle`.

### 11.2 Reconnect

WS dies → supervisor flips every instance to `Reconnecting` → backoff → re-open → re-Hello for everyone. Cloud treats fresh `Hello` on a new connection as "this runner is back" — same as today. In-flight runs are not lost: the per-instance `RunnerLoop` keeps running while the WS is down (`AssignWorker` doesn't depend on connection state to drive the agent, only to send progress frames), and emits `RunResumed` once its `Welcome` arrives.

### 11.3 Resume

Today the runner sends `RunResumed` after reconnect when it has an in-flight run. With N instances, after reconnect each instance independently emits `RunResumed { runner_id }` (in its envelope) if it has one. The demux is one-way for resumes — outbound only.

### 11.4 Removing a runner

Two entry points, same effect:

- **Cloud-initiated**: cloud sends `Envelope { runner_id: Some(id), body: RemoveRunner { reason } }` (e.g. user clicked "Remove" in the runners section of the UI).
- **Locally**: `pidash remove --runner foo` over IPC.

Either way, the supervisor:

1. Cancels that instance's in-flight run (if any) — same hard-cancel path as token revocation (5s grace, then SIGKILL).
2. Removes the instance from `instances`.
3. Drops the mailbox.
4. Frees its working directory binding (the directory itself is left on disk; user can reclaim manually).
5. **Deletes the runner's local data directory** (`data_dir/runners/<runner_id>/`) — history, logs, identity file. Once removed, the runner's data is gone.

If the trigger was local (`pidash remove --runner`), the CLI additionally calls `POST /api/v1/runner/<runner_id>/deregister/` over REST (authenticated with the token) before it asks the daemon to remove the instance, so cloud-side `Token.owns` and any in-flight assignments are cleaned up authoritatively. **No `Bye` frame is sent over the WS** — `Bye` is reserved for connection teardown (§4.2). If the trigger was cloud-initiated `RemoveRunner`, no REST call is needed (cloud already initiated the removal).

The connection stays up; other instances and the token are unaffected.

### 11.5 Token revocation

Cloud-initiated, surfaced via the **Revoke** action in the tokens section of the UI. Effect: the token's `secret` is invalidated cloud-side immediately, so the next reconnect (or any auth check) fails.

**One contract, regardless of connection state**: revocation always terminates the daemon. Specifically:

- **If the connection is up**: cloud sends connection-scoped `Revoke { reason }` (no `runner_id`). Daemon hard-cancels every in-flight run (5s grace, then SIGKILL via the standard `bridge.shutdown` path), sends `Bye { reason: "token revoked" }`, and exits with a non-zero status.
- **If the connection is down at revoke time**: daemon discovers it on the next reconnect attempt, when the WS upgrade returns 401. Daemon hard-cancels every in-flight run, logs an explicit error (`"token revoked or invalid; daemon exiting. Run 'pidash configure token' to install a new token."`), and exits with a non-zero status.

In both cases the process exits. There is no `AwaitingReauth` state at the connection/token level — `AwaitingReauth` is a per-runner status used by the agent's own reauth flow (e.g. Codex needing the user to log in again), not by token-level auth failures. The systemd/launchd unit is configured with `Restart=on-failure`, so a transient 5xx during reconnect (not a 401) will see the daemon retry; only an authoritative auth failure causes the exit.

Once the daemon exits, recovery is: user runs `pidash configure token` to install a fresh token, then `pidash start` (or the service manager auto-restarts on the next boot).

There is no per-runner Revoke. Revocation is a security action against a credential; if a single runner needs to go away, that's a Remove.

### 11.6 Adding a runner at runtime

`pidash configure runner --name foo` → CLI registers via REST under the active token → IPC tells daemon "load instance foo" → supervisor inserts into `instances`, sends `Hello { runner_id: foo }` over the existing WS, awaits `Welcome`, spawns the instance's `RunnerLoop`. No reconnect.

### 11.7 AwaitingReauth

Per-instance, not daemon-global. Today's single global `RunnerStatus` becomes per-instance — only the affected runner's `RunnerStateHandle.status` flips to `AwaitingReauth`, and only that runner's heartbeat reflects it. Sibling runners on the same connection keep working.

Token-level reauth (the `Bearer token_secret` itself becomes invalid) is a different beast — see §11.5.

## 12. Failure semantics

| Scenario                                             | Behaviour                                                                                                                                       |
| ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| WS connection drops                                  | All instances → `Reconnecting`. In-flight runs continue locally. Reconnect re-Hellos for everyone.                                              |
| One agent subprocess crashes                         | Only that instance's run fails (`AgentCrash` / `CodexCrash`). Other instances unaffected.                                                       |
| One instance Removed                                 | That instance torn down. Connection and other instances unaffected.                                                                             |
| Token Revoked                                        | Connection torn down on next auth check. Daemon shuts down all instances.                                                                       |
| Cloud sends frame for unknown `runner_id`            | Demux logs a warning and drops. Connection stays up.                                                                                            |
| `Hello` rejected (token doesn't own that runner)     | Cloud sends per-runner `RemoveRunner { runner_id }`. That instance is removed from `instances` with a clear log line; other instances continue. |
| Two instances configured with the same `working_dir` | Detected at startup, daemon refuses to start with a clear error.                                                                                |
| Heartbeat task back-pressure (out_tx full)           | Single shared `out_tx` — same buffer (128) as today. Worth raising to ~512 with N instances since heartbeats now multiply.                      |

The one explicit cost of shared transport: a WS hiccup briefly stalls _all_ instances at once. Acceptable trade-off given the design's other goals; not a blocker.

## 13. Rollout

Pi Dash has no production runner users yet, so there is no v1→v2 migration story to support. Cloud and runner ship v2 only. v1 auth (`X-Runner-Id` + `Bearer runner_secret`) is not implemented on the cloud side at all.

### 13.1 Cloud-side

1. Roll the wire protocol (envelope `runner_id`), `Welcome { runner_id }` per `Hello`, and the `RemoveRunner` variant.
2. Add `Token` entity (§5.1), registration endpoints, UI tokens section, per-runner Remove action.
3. Update assigner so `Assign` is keyed by `runner_id` independent of which connection currently holds that runner.

### 13.2 Runner-side (only path)

1. User creates a token via the cloud UI, which generates a setup script with `token_id` + `token_secret` baked in.
2. User runs the script on the dev machine. Script installs `pidash` (if needed), writes `[token]` block to `credentials.toml`, creates the first runner with the values the user specified in the UI form, starts the daemon.
3. Daemon connects with `X-Token-Id` + `Bearer token_secret`, sends `Hello { runner_id }` per configured runner, comes up.

See §10.x (user journeys) for the full end-to-end flow.

### 13.3 Stale-config refusal

If the daemon starts against a `credentials.toml` that exists but has no `[token]` block (e.g. a stale dev install from before this design), it refuses to start with a clear error:

```
no token configured: credentials.toml has no [token] block.
Run 'pidash configure --token-id <id> --token-secret <secret>' (or paste the
setup script from the cloud UI) to install a token.
```

There is no automatic in-place upgrade path. Stale configs are decommissioned by deleting `credentials.toml` (and `data_dir/runners/*` if present) and running fresh setup.

## 14. Files most affected

| File                                                         | Change                                                                                                                                                            |
| ------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `runner/src/cloud/protocol.rs`                               | `Envelope` adds `runner_id`; `WIRE_VERSION = 2`; per-runner-vs-connection-scoped routing rules documented in code.                                                |
| `runner/src/cloud/ws.rs`                                     | Auth headers switch to `X-Token-Id` + `Bearer token_secret`. Connect→Hello loop iterates instances. Inbound stream feeds Demux instead of a single mpsc consumer. |
| `runner/src/cloud/register.rs`                               | Adds token registration (`pidash configure token`); runner registration takes a token context (`token_id` becomes a parameter).                                   |
| `runner/src/daemon/supervisor.rs`                            | `Supervisor` owns `instances` map. `RunnerLoop` becomes per-instance, spawned N times. Demux task added. Heartbeat task iterates instances.                       |
| `runner/src/daemon/state.rs`                                 | `StateHandle` splits into `ConnectionStateHandle` (per-daemon) and `RunnerStateHandle` (per-instance).                                                            |
| `runner/src/ipc/protocol.rs`                                 | `StatusSnapshot` carries `Vec<RunnerStatusSnapshot>`. IPC verbs gain `runner` selector. Bump IPC version.                                                         |
| `runner/src/ipc/server.rs`                                   | Routes commands by `runner` selector to the right `RunnerInstance`.                                                                                               |
| `runner/src/config/schema.rs`                                | Top-level config gains `[daemon]` + `[[runner]]` array. Migration path from v1 shape.                                                                             |
| `runner/src/util/paths.rs`                                   | Adds `runner_dir(runner_id)` and per-instance `RunnerPaths`.                                                                                                      |
| `runner/src/cli/configure.rs`                                | Splits into `configure token` (one-shot per host) and `configure runner --name` (per instance).                                                                   |
| `runner/src/cli/{status,issue,comment,tui,remove,doctor}.rs` | All gain a `--runner` selector.                                                                                                                                   |
| `runner/src/approval/router.rs`                              | Unchanged shape, just instantiated per instance.                                                                                                                  |
| `runner/src/history/{jsonl,index}.rs`                        | Take `RunnerPaths` instead of a global `Paths`.                                                                                                                   |

## 15. ADR — N WebSocket sessions rejected

**Considered**: keep the wire protocol untouched, run N independent `ConnectionLoop`s in one daemon process, each with its own `Credentials`. Cloud sees N independent runners, identical to running N separate daemons today.

**Why rejected**:

- At fleet scale, N idle WSes per dev machine consume cloud-side connection slots and TLS state proportional to N × machines. With shared connection that drops to 1 × machines.
- Heartbeat traffic to cloud scales N×; with shared connection it still fan-outs to N envelopes per tick but uses one TCP stream's congestion control.
- N separate auth contexts to rotate, monitor, and audit per host. A `Machine` credential gives the cloud a single host-scoped identity to reason about.

**Why considered (and acknowledged as cheaper to build)**:

- Zero protocol changes. Zero cloud changes. Zero auth-model changes.
- Better failure isolation: one runner's WS hiccup doesn't stall the others.
- Could ship in a single PR.

The shared-connection design wins on long-term cost (cloud connection count, audit/rotation cleanliness) at the price of a meaningful one-time engineering investment on both sides. Failure isolation is the real loss; mitigated by fast reconnect and the fact that runs continue locally during a connection blip (§11.2).

**Decision is final.** Cloud team has committed to the v2 protocol and the token (machine credential) entity. The N-WS variant is no longer treated as a fallback option; this section is retained as a record of the trade-off considered.

## 16. Instance count cap

Default cap: **50 instances per daemon**. Both the daemon-side validation (refuses to start if `config.toml` lists more) and the cloud-side enforcement (rejects `Hello` beyond the cap) use this number.

50 is well above any plausible legitimate use (a dev box with 8 cores can productively serve maybe 4–6 concurrent codex agents) and well below "unbounded" — the cap is a foot-gun guard and an abuse mitigation, not a tuning knob.

Cap can be revisited later. Lower it (e.g. 8) if 50 turns out to mask configuration mistakes; raise it if multi-runner sees real fleet-style usage. For now, 50 is the published default.
