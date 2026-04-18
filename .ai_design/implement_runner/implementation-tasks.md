# Pi Dash Runner — Implementation Tasks

Purpose: track the implementation of the Pi Dash local runner, cloud integration, and built-in TUI.

Companion docs in this directory:

- `runner-design.md`
- `github-runner-architecture.md`
- `tui-design.md`

How to use this file:

- Keep task status in-place with checkboxes.
- Add PR links or issue ids inline after the task text.
- Do not delete completed tasks; strike or annotate only if scope changes.
- If a task expands materially, split it into a separate subtask block in this file.

## Milestones

- [ ] Phase 0: one-user happy-path proof of concept
- [ ] Phase 1: Codex bridge and local persistence
- [ ] Phase 2: approvals end to end
- [ ] Phase 3: production-grade runner lifecycle
- [ ] Phase 4: TUI MVP complete
- [ ] Phase 5: hardening and rollout

## 0. Project setup

- [ ] Decide the implementation repo layout
      Notes: confirm whether the runner lives in this monorepo as a Rust workspace member, a sibling directory under `apps/`, or a top-level `runner/` directory.
- [ ] Create the runner project scaffold
      Notes: include build, test, fmt, lint, and release targets.
- [ ] Choose the Rust crate baseline
      Notes: likely `tokio`, `reqwest`, `tokio-tungstenite` or equivalent, `serde`, `toml`, `tracing`, `ratatui`, `crossterm`, `clap`, `directories`.
- [ ] Define versioning strategy
      Notes: semver for runner binary and explicit protocol version for runner-cloud messages.
- [ ] Add local developer commands to repo docs or AGENTS-facing docs
      Notes: build runner, run daemon, run TUI, run tests.

## 1. Cloud contract

### 1.1. Registration and auth

- [ ] Define registration endpoint request/response schema
      Notes: input token, runner name, OS, arch, version; output runner id, runner secret, heartbeat interval, protocol version.
- [ ] Define runner credential storage format
      Notes: separate config from secret-bearing credentials.
- [ ] Define reconnect authentication flow
      Notes: runner reconnects with long-lived credential and receives short-lived session acceptance.
- [ ] Define credential revocation behavior
      Notes: what happens when the server revokes a runner while it is connected.
- [ ] Define credential rotation behavior
      Notes: whether rotation is push-driven, pull-driven, or manual in v1.

### 1.2. WebSocket protocol

- [ ] Write message schema for `hello`
- [ ] Write message schema for `hello_ack`
- [ ] Write message schema for `heartbeat`
- [ ] Write message schema for `assign_run`
- [ ] Write message schema for `run_event`
- [ ] Write message schema for `approval_requested`
- [ ] Write message schema for `approval_decision`
- [ ] Write message schema for `cancel_run`
- [ ] Write message schema for `run_completed`
- [ ] Define message idempotency rules
      Notes: retries must not create duplicate runs, duplicate approvals, or duplicate completion processing.
- [ ] Define event ordering rules
      Notes: monotonic `seq` per run, reconnect replay expectations, server handling for gaps.

### 1.3. Lifecycle defaults

- [ ] Finalize heartbeat interval
- [ ] Finalize offline timeout
- [ ] Finalize reconnect grace window
- [ ] Finalize approval timeout
- [ ] Finalize server behavior when runner disconnects mid-run
- [ ] Finalize server behavior when approval times out

## 2. Django data model and APIs

### 2.1. Models

- [ ] Add `Runner` model
      Notes: owner, workspace, name, status, capabilities, credential fingerprint, version, OS, arch, last heartbeat.
- [ ] Add `RunnerRegistrationToken` model
      Notes: store token hash only, expiration, consumed state.
- [ ] Add `AgentRun` model
      Notes: run config, required capabilities, lease expiry, thread id, lifecycle timestamps, error field.
- [ ] Add `AgentRunEvent` model or equivalent transcript storage
      Notes: confirm bounded retention approach before shipping full event persistence.
- [ ] Add `ApprovalRequest` model
      Notes: pending/accepted/declined, payload, expiry, decision source.
- [ ] Add migrations and model tests

### 2.2. Runner APIs

- [ ] Implement runner registration API
- [ ] Implement runner deregistration API
- [ ] Implement runner reconnect/session-auth API
- [ ] Implement runner heartbeat update API or WS-backed equivalent
- [ ] Implement runner revocation path
- [ ] Implement admin/user API to mint registration tokens

### 2.3. Run APIs

- [ ] Implement API to create `AgentRun` from a work item
- [ ] Implement API to cancel an `AgentRun`
- [ ] Implement API to inspect run status and transcript summary
- [ ] Implement API to fetch recent runs for a runner
- [ ] Implement API to fetch pending approvals
- [ ] Implement API to answer approvals from web UI

## 3. Realtime service

### 3.1. Service scaffold

- [ ] Create a dedicated runner WS service
      Notes: reuse `apps/live` deployment patterns, not its auth model.
- [ ] Add service configuration for Redis, ports, secrets, and base URL
- [ ] Add health endpoint
- [ ] Add structured logging
- [ ] Add graceful shutdown behavior

### 3.2. Runner connection handling

- [ ] Implement `hello` handshake validation
- [ ] Track connected runner sessions in memory
- [ ] Persist runner presence to shared state
- [ ] Implement heartbeat handling
- [ ] Mark runner offline on timeout
- [ ] Reject protocol-version mismatches cleanly

### 3.3. Pub/sub and routing

- [ ] Implement Redis pub/sub for runner-targeted messages
- [ ] Implement routing from Django/orchestrator to connected runner socket
- [ ] Implement reconnect-safe session remapping
- [ ] Implement dedupe for duplicate inbound runner messages

### 3.4. Run control

- [ ] Implement `assign_run` delivery to runner
- [ ] Implement `cancel_run` delivery to runner
- [ ] Implement `approval_decision` delivery to runner
- [ ] Implement `run_completed` ingestion from runner
- [ ] Implement `approval_requested` ingestion from runner

## 4. Scheduling and orchestration

- [ ] Define how a work item becomes an `AgentRun`
- [ ] Implement runner selection query for v1
      Notes: one user-owned online idle runner; no label matching in MVP if following `runner-design.md`.
- [ ] Implement run lease acquisition
- [ ] Prevent concurrent assignment to one runner
- [ ] Requeue or fail runs that are not accepted within the acceptance window
- [ ] Implement cancellation propagation from cloud to runner
- [ ] Implement reconnect resume logic for in-flight runs

## 5. Runner daemon

### 5.1. CLI and process model

- [ ] Implement CLI entrypoints
      Notes: `configure`, `start`, `service install`, `service start`, `service stop`, `status`, `doctor`, `tui`.
- [ ] Implement daemon main loop
- [ ] Implement config and credential file loading
- [ ] Enforce file permissions for secrets and local IPC
- [ ] Implement structured logging and log rotation policy

### 5.2. Service integration

- [ ] Implement macOS `launchd` install/start/stop
- [ ] Implement Linux `systemd --user` install/start/stop
- [ ] Add service status diagnostics

### 5.3. Cloud connection

- [ ] Implement registration flow
- [ ] Implement WS connect and `hello`
- [ ] Implement heartbeat loop
- [ ] Implement jittered exponential reconnect
- [ ] Implement reconnect-state reporting for in-flight run
- [ ] Handle revocation and forced disconnect

### 5.4. Workspace management

- [ ] Implement working-dir config
- [ ] Detect existing git repo in working dir
- [ ] Handle empty-dir clone path
- [ ] Refuse non-empty non-repo working dir
- [ ] Capture and report clone/auth/network failures clearly
- [ ] Persist workspace resolution details in local run history

### 5.5. Local state machine

- [ ] Implement runner states
      Notes: idle, assigned, running, awaiting approval, reconnecting, awaiting reauth.
- [ ] Enforce one active run at a time
- [ ] Implement run lease expiry handling
- [ ] Implement graceful shutdown during active run

## 6. Codex bridge

### 6.1. Subprocess management

- [ ] Spawn `codex app-server` with configured working directory
- [ ] Capture stdout/stderr and parse JSON-RPC messages
- [ ] Implement clean startup timeout handling
- [ ] Implement clean shutdown and forced kill fallback
- [ ] Handle Codex crash and one-time resume attempt

### 6.2. Run execution

- [ ] Translate `assign_run` into Codex thread/turn start calls
- [ ] Persist `thread_id` for resume
- [ ] Handle Codex completion message
- [ ] Handle Codex failure message
- [ ] Forward only lifecycle events to cloud per MVP privacy rules
- [ ] Keep `item/*` deltas local only

### 6.3. Reauth path

- [ ] Detect Codex auth failure / reauth-needed signal
- [ ] Move runner/run into `awaiting_reauth`
- [ ] Notify TUI and web surfaces
- [ ] Resume run after successful reauth if supported
- [ ] Fail run cleanly if reauth does not happen within timeout

## 7. Approval flow

### 7.1. Policy engine

- [ ] Implement read-only auto-approve allowlist
- [ ] Implement always-deny destructive action list
- [ ] Implement config-driven approval policy
- [ ] Add tests for policy classification

### 7.2. Approval routing

- [ ] Persist approval request locally and in cloud
- [ ] Notify local TUI subscribers
- [ ] Notify web UI via cloud
- [ ] Accept decision from either surface
- [ ] Deduplicate first-writer-wins decision handling
- [ ] Return decision to Codex
- [ ] Expire stale approvals and cancel run by default

## 8. Local persistence and privacy

- [ ] Define local run-history directory layout
- [ ] Write JSONL or equivalent append-only local event log
- [ ] Persist final transcript summary per run
- [ ] Bound retention by age or size
- [ ] Ensure cloud only receives allowed lifecycle data in MVP
- [ ] Add redaction rules for logs if needed

## 9. TUI client

### 9.1. Local IPC

- [ ] Implement Unix-socket IPC on macOS/Linux
- [ ] Define JSON-RPC methods for TUI attachment
- [ ] Implement `status.get`
- [ ] Implement `status.subscribe`
- [ ] Implement `config.get`
- [ ] Implement `config.update`
- [ ] Implement `runs.list`
- [ ] Implement `runs.get`
- [ ] Implement `approvals.list`
- [ ] Implement `approvals.decide`
- [ ] Implement `doctor.run`

### 9.2. Views

- [ ] Build Status view
- [ ] Build Runs list view
- [ ] Build Run detail view
- [ ] Build Config view
- [ ] Build Approvals view
- [ ] Build Help overlay

### 9.3. UX flows

- [ ] Implement first-run onboarding wizard
- [ ] Implement approval focus-jump and bell behavior
- [ ] Implement quit vs stop-daemon confirmation
- [ ] Implement refresh and search interactions

## 10. Web UI

- [ ] Add runner registration-token creation UI
- [ ] Add runner status UI
- [ ] Add current run detail UI
- [ ] Add recent runs UI
- [ ] Add approval inbox UI
- [ ] Add approval decision actions
- [ ] Add runner revoke/disconnect controls

## 11. Observability and ops

- [ ] Define runner logs schema
- [ ] Define WS service logs schema
- [ ] Add metrics for connected runners, heartbeats, active runs, approval latency
- [ ] Add alerts for offline runners and repeated reconnect failures
- [ ] Add basic admin troubleshooting guide

## 12. Security review

- [ ] Verify secret files are written with correct permissions
- [ ] Verify local IPC is user-only
- [ ] Verify runner auth is not cookie-based and does not reuse `apps/live` shared-secret model
- [ ] Verify approval requests cannot be forged or replayed easily
- [ ] Verify cloud-scoped credentials are least-privilege
- [ ] Verify local event logs do not leak unnecessary secrets

## 13. Testing

### 13.1. Unit tests

- [ ] Runner config parsing
- [ ] Credential storage and permissions
- [ ] WS message encoding/decoding
- [ ] Reconnect backoff behavior
- [ ] Workspace resolution logic
- [ ] Approval policy classification
- [ ] Local state-machine transitions

### 13.2. Integration tests

- [ ] Registration end to end
- [ ] Runner connect and heartbeat end to end
- [ ] Assign-run happy path end to end
- [ ] Approval round-trip from Codex to TUI/web and back
- [ ] Cancellation path end to end
- [ ] Disconnect and reconnect resume path
- [ ] Codex reauth path

### 13.3. Manual validation matrix

- [ ] macOS arm64 install to first successful run
- [ ] macOS x64 install to first successful run
- [ ] Linux x64 install to first successful run
- [ ] Existing repo working-dir flow
- [ ] Empty-dir clone flow
- [ ] Clone-auth failure flow
- [ ] Laptop sleep/wake during active run
- [ ] TUI attach/detach during active run

## 14. Release readiness

- [ ] Build signed release artifacts for supported platforms
- [ ] Publish install instructions
- [ ] Publish operator/admin guide
- [ ] Publish end-user setup guide
- [ ] Roll out to internal users first
- [ ] Collect feedback and bug list
- [ ] Decide go/no-go for broader rollout

## 15. Deferred work

- [ ] Windows support
- [ ] Automatic clone/worktree management beyond `require_existing`
- [ ] Multi-runner or multi-slot concurrency per machine
- [ ] Rich transcript sync to cloud
- [ ] Diff preview in TUI
- [ ] Native tray icon
- [ ] Auto-update flow
- [ ] Containerized per-run isolation
