# Multi-Runner Daemon — Tasks

Purpose: track the implementation of hosting N runners under one daemon over a single multiplexed WebSocket session.

Companion docs in this directory:

- `design.md` — architecture
- `decisions.md` — resolved design questions
- `implementation-plan.md` — phased rollout strategy

How to use this file:

- Keep task status in-place with checkboxes.
- Add PR links or issue ids inline after the task text.
- Do not delete completed tasks; strike or annotate only if scope changes.
- If a task expands materially, split it into a separate subtask block.

## Milestones

- [ ] Phase 1: cloud wire protocol with envelope `runner_id`
- [ ] Phase 2: cloud token entity + UI tokens-vs-runners split
- [ ] Phase 3: runner persistence + config layout
- [ ] Phase 4: runner per-instance state types
- [ ] Phase 5: runner connection multiplex
- [ ] Phase 6: lift the single-tenant cap
- [ ] Phase 7: runtime add + cloud-initiated remove + token revocation

## 1. Phase 1 — Cloud: wire protocol

### 1.1. Envelope shape

- [ ] Add optional `runner_id: Option<Uuid>` to the wire envelope schema
      Notes: `design.md` §4.1; cloud and runner serializers updated together.
- [ ] Update protocol round-trip tests to cover the new envelope shape
- [ ] Define rejection behavior for frames with missing `runner_id` where required
      Notes: see §4.3 routing rule.

### 1.2. Frame routing rules

- [ ] Implement demux dispatch by `Envelope.runner_id`
      Notes: `{ Ping, Bye, Revoke }` → connection consumer; everything else → instance mailbox.
- [ ] Add `RemoveRunner { run_id?: Uuid, reason: Option<String> }` to `ServerMsg`
      Notes: per-runner; cloud sends to evict one instance.
- [ ] Make `Welcome` per-runner (one per `Hello`)
      Notes: §4.3; no separate connection-level Welcome.
- [ ] Move `Ping` handling out of `RunnerLoop` into the connection task
      Notes: today handled inline in `runner/src/daemon/supervisor.rs:327`.

### 1.3. Tests

- [ ] Integration test: frame with missing `runner_id` is rejected without dropping connection
- [ ] Integration test: frame for unknown `runner_id` is dropped with a warning, connection stays up
- [ ] Integration test: per-runner Welcome correctly delivered to the matching instance

## 2. Phase 2 — Cloud: token entity + UI

### 2.1. Schema

- [ ] Add `tokens` table (`token_id`, `secret_hash`, `title`, `workspace_id`, `created_at`, `last_seen_at`, `revoked_at`)
- [ ] Add `runners.token_id` FK (NOT NULL)
- [ ] Cloud-side validation: token title required at creation, free-text within a workspace
      Notes: open question — token title uniqueness within workspace? Decide before shipping.

### 2.2. Auth

- [ ] WS upgrade accepts `X-Token-Id` + `Bearer token_secret` headers
      Notes: validates against the token row; populates connection's authorised set with `Token.owns`.
- [ ] Reject `Hello { runner_id }` for runners not in the token's `owns` set
      Notes: cloud responds with per-runner `RemoveRunner`.

### 2.3. REST endpoints

- [ ] `POST /api/v1/token/register/`
      Notes: request body `{ title }`; response `{ token_id, token_secret }`; secret shown once.
- [ ] `POST /api/v1/runner/register/`
      Notes: requires `token_id`; auth `Bearer token_secret`; request body `{ name, working_dir, agent, approval_policy }`; response `{ runner_id }`.
- [ ] `POST /api/v1/runner/<runner_id>/deregister/`
      Notes: auth `Bearer token_secret`; called by `pidash remove --runner <name>`.
- [ ] Setup-script generation endpoint (UI-facing)
      Notes: generates the curl-pipe-sh installer for the new-connection path with values inlined.

### 2.4. UI

- [ ] Tokens section: list tokens by title with associated runners; per-token Revoke action
      Notes: `design.md` §5.3.
- [ ] Runners section: list runners under tokens; per-runner Remove action; no per-runner Revoke
- [ ] Add Runner modal with two paths:
      - [ ] New connection: connection title + runner name + working dir + agent + approval policy form
      - [ ] Existing connection: dropdown of tokens + same fields minus title
- [ ] Generated setup script copy/download UI for both paths

## 3. Phase 3 — Runner: persistence + config layout

### 3.1. Paths

- [ ] Add `RunnerPaths` newtype carrying a baked-in `runner_id`
      Notes: `runner/src/util/paths.rs`.
- [ ] `Paths::runner_dir(runner_id) -> PathBuf` returns `data_dir/runners/<runner_id>/`
- [ ] Migrate `HistoryWriter`, `RunsIndex`, log paths to take `RunnerPaths` instead of `Paths`

### 3.2. Config schema

- [ ] Split `Config` into `DaemonConfig` (cloud_url, heartbeat_interval_secs, log_level) + `Vec<RunnerConfig>`
      Notes: `runner/src/config/schema.rs`.
- [ ] `RunnerConfig` owns `name`, `runner_id`, `agent`, `workspace`, `approval_policy`
- [ ] Daemon refuses to start if `credentials.toml` exists but has no `[token]` block
      Notes: `design.md` §13.3.
- [ ] Daemon refuses to start on duplicate `working_dir`, `runner_id`, `name` across `[[runner]]` entries
      Notes: §9 validation rules; nested working_dirs (one is a prefix of another) also rejected.
- [ ] Daemon refuses to start if instance count > 50

### 3.3. Credentials schema

- [ ] `Credentials` carries `[token]` block (token_id, token_secret, title) + `[[runner]]` array (runner_id, name) + `api_token`
- [ ] `runner_secret` field removed
- [ ] `credentials.toml` written with mode 0600

## 4. Phase 4 — Runner: per-instance state types

### 4.1. State split

- [ ] Split `StateHandle` into `ConnectionStateHandle` (per-daemon) + `RunnerStateHandle` (per-instance)
      Notes: `runner/src/daemon/state.rs`.
- [ ] `RunnerStateHandle` owns per-instance `current_run`, `tx_in_flight`, `approvals_pending`, `runner_id`, `status`
- [ ] `ConnectionStateHandle` owns `connected`, `last_heartbeat_ack`, `authorised_runners: HashSet<Uuid>`
- [ ] Move `AwaitingReauth` from global runner status to per-instance status

### 4.2. RunnerInstance

- [ ] Define `RunnerInstance` struct (see `design.md` §6.2)
- [ ] Per-instance `ApprovalRouter`
- [ ] Per-instance `RunnerPaths`
- [ ] Per-instance mailbox `mpsc::Sender<Envelope<ServerMsg>>`

### 4.3. RunnerLoop reshape

- [ ] `RunnerLoop` takes a `RunnerInstance` instead of supervisor-wide handles
      Notes: `runner/src/daemon/supervisor.rs:184`.
- [ ] `RunnerLoop` no longer dispatches `Ping` or `Revoke` (moved to connection task)
- [ ] `RunnerLoop` dispatches `RemoveRunner`

### 4.4. IPC StatusSnapshot

- [ ] `StatusSnapshot` carries `{ daemon: DaemonInfo, runners: Vec<RunnerStatusSnapshot> }`
      Notes: `runner/src/ipc/protocol.rs`; bump IPC version.
- [ ] TUI and CLI consumers updated to read the new shape
- [ ] `RunnerStatusSnapshot` carries runner_id, name, status, current_run, approvals_pending

## 5. Phase 5 — Runner: connection multiplex

### 5.1. Envelope + frames

- [ ] `Envelope` gains `runner_id` field
- [ ] `RunnerOut` newtype wraps shared `out_tx`; sets `runner_id` automatically
      Notes: `design.md` §6.3.
- [ ] Replace every `out.send(Envelope::new(…))` site to use `RunnerOut::send`

### 5.2. Demux task

- [ ] Demux task between `ConnectionLoop`'s inbound mpsc and per-instance mailboxes
      Notes: `design.md` §6.4.
- [ ] Connection-scoped frames routed to supervisor inbox
- [ ] Frames for unknown runner_ids dropped with a warning

### 5.3. Multi-Hello / Welcome handling

- [ ] `ConnectionLoop` walks `instances` after WS upgrade and sends `Hello { runner_id }` for each
- [ ] Per-instance `Welcome { runner_id }` flips that instance from `Reconnecting` to `Idle`
- [ ] Connection-level fields in Welcome (protocol_version, heartbeat_interval) accepted from first Welcome, propagated via watch channel

### 5.4. Heartbeats

- [ ] Per-runner heartbeat task fan-out: one envelope per instance per tick
      Notes: `design.md` §6.6.
- [ ] Connection-level pong: one reply per cloud-sent `Ping`
- [ ] Buffer size for shared `out_tx` raised to 512

### 5.5. Auth

- [ ] WS connects with `X-Token-Id` + `Bearer token_secret` from `[token]` block

### 5.6. Tests

- [ ] Property-based test for demux: frames always reach the right consumer
- [ ] Manual run with `RUST_LOG=trace` against fake cloud during PR review
- [ ] Reconnect test: each instance independently emits `RunResumed` after reconnect

## 6. Phase 6 — Runner: lift the cap

### 6.1. Multi-instance hosting

- [ ] Allow `Vec<RunnerConfig>` of arbitrary length up to cap
- [ ] Supervisor spawns N `RunnerLoop`s

### 6.2. CLI selectors

- [ ] `pidash status` lists all instances; `pidash status --runner <name>` for one
- [ ] `pidash configure` splits into:
      - [ ] `pidash configure token` (one-shot per host)
      - [ ] `pidash configure runner --name <name>` (per instance, registers under active token)
- [ ] `pidash issue`, `pidash comment`, `pidash state`, `pidash workspace` gain `--runner` selector
      Notes: required when N > 1; optional when N = 1.
- [ ] `pidash doctor --runner <name>` checks one instance; bare `pidash doctor` walks all

### 6.3. `pidash remove` disambiguation

- [ ] Bare `pidash remove`: full machine teardown (today's behavior — preserved)
      Notes: stops service, uninstalls unit, deregisters token, deletes config + creds + data.
- [ ] `pidash remove --runner <name>`: per-instance removal via REST deregister + IPC eviction
      Notes: `design.md` §10.1.5; no WS frame emitted.

### 6.4. TUI

- [ ] Instance picker / multi-pane view (deferred — separate design doc)
      Notes: §E in earlier scoping; placeholder UI in phase 6 if full TUI redesign isn't ready.

## 7. Phase 7 — Runtime add + cloud-initiated remove + token revocation

### 7.1. Runtime add

- [ ] `pidash configure runner --name foo` over IPC tells daemon to load instance
- [ ] Supervisor inserts into `instances`, sends `Hello`, spawns `RunnerLoop`, awaits `Welcome`
- [ ] No reconnect

### 7.2. Cloud-initiated remove

- [ ] `RemoveRunner { runner_id, reason }` handler in supervisor
- [ ] Cancel in-flight run for that runner (5s grace, then SIGKILL)
- [ ] Remove instance from `instances`, drop mailbox, delete `data_dir/runners/<runner_id>/`
- [ ] Connection and other instances stay up

### 7.3. Token revocation

- [ ] Connection-scoped `Revoke { reason }` handler in connection task
- [ ] Daemon hard-cancels every in-flight run, sends `Bye`, exits non-zero
- [ ] Async path: 401 on reconnect → daemon hard-cancels in-flight runs, logs explicit error, exits non-zero
- [ ] systemd/launchd unit configured with `Restart=on-failure` for transient errors only

## 8. Cross-cutting work

### 8.1. Observability

- [ ] Per-instance `tracing::Span` scoped to `RunnerLoop`; every log line carries `runner_id`
- [ ] Metrics gain `runner_id` label
- [ ] New gauge `pidash_instances_count` on the daemon

### 8.2. Doctor

- [ ] `pidash doctor` walks each instance: check working_dir, agent install, credentials independently

### 8.3. Stale-config error message

- [ ] "no token configured: credentials.toml has no [token] block. Run 'pidash configure --token-id <id> --token-secret <secret>' …"

### 8.4. Tests

- [ ] Extend `protocol_roundtrip.rs` for the new envelope shape and new ServerMsg/ClientMsg variants
- [ ] Update `cloud_ws_fake.rs` to speak the new protocol (multi-Hello, per-runner Welcome, RemoveRunner)
- [ ] Integration test: two-instance daemon, fake cloud, simultaneous assignments
- [ ] Integration test: misconfigured daemon (duplicate working_dir) refuses to start with clear message

## 9. Open questions / deferred

- [ ] Token title uniqueness within a workspace — required or not?
- [ ] Direct-edit reload behavior for `config.toml` — SIGHUP, file watcher, or explicit `pidash configure --reload`?
      Notes: `design.md` §10.1.4 mentions explicit reload.
- [ ] `--purge-workspace` flag on `pidash remove --runner` to nuke the working_dir on disk
- [ ] Per-runner log level override — deferred (today logging is daemon-level)
- [ ] Multi-runner TUI layout — separate design doc, gates phase 6 polish
- [ ] WS+REST auth unification — separate scoping conversation; out of scope here (decisions Q9)
