# Daemon-Side Module Design — Per-Runner HTTPS Long-Poll Transport

> Companion to `.ai_design/move_to_https/design.md`. That doc
> specifies the wire protocol and cloud-side implementation. This doc
> specifies how the new transport plugs into the existing daemon
> architecture from `.ai_design/n_runners_in_same_machine/`.
>
> Concretely: how `runner/src/cloud/http.rs` (new) replaces
> `runner/src/cloud/ws.rs`'s role in `Supervisor::run` while preserving
> the per-`RunnerInstance` boundary. The shape is **per-runner clients,
> shared transport** (per design.md §1 + §3 decision #3, decision #10):
> each `RunnerInstance` owns its own auth state machine, refresh
> schedule, session, and poll loop; the daemon process holds a single
> shared `reqwest::Client` so all runners benefit from one TCP / HTTP/2
> keep-alive pool.

## 1. What stays, what changes

The daemon's existing process-level decomposition is **kept**:

- `Supervisor` (`runner/src/daemon/supervisor.rs`) still owns the
  lifecycle of N `RunnerInstance`s, the IPC handle, and process-level
  shutdown coordination. **What it no longer owns**: a shared cloud
  identity, a shared session, a shared long-poll loop, or a
  cross-runner message demux. Those move down into each runner.
- `RunnerInstance` (`runner/src/daemon/runner_instance.rs`) is
  unchanged in shape. It continues to own per-runner state
  (`StateHandle`, `ApprovalRouter`, mailbox sender/receiver pair,
  paths, config). It **gains**: its own `RunnerCloudClient` (auth
  state machine + cloud transport methods) and its own `HttpLoop`
  (per-runner long-poll task).
- `RunnerLoop` (`supervisor.rs:431`) — per-instance task that
  consumes from its mailbox, drives the agent bridge, posts upstream
  events — is unchanged in shape. Its inputs and outputs are
  reshaped, but the loop body is the same.
- `RunnerOut` (`runner/src/daemon/runner_out.rs`) — the per-instance
  send wrapper that callers in `RunnerLoop`/`AssignWorker` use — is
  unchanged at the call site. Its **internal dispatch** changes (see
  §3): variant-→-URL mapping per runner instead of mpsc-into-WS.
  Every existing `out.send(ClientMsg::...)` call site recompiles.

What goes away:

- `runner/src/cloud/ws.rs::ConnectionLoop` is no longer dialed from
  `Supervisor::run`. The `ws.rs` module stays in the build for the
  per-run upgrade ticket path described in `design.md` §7.9.
- The per-runner heartbeat tasks (`hb_handles` in
  `supervisor.rs:251`) go away. Status (`status`, `in_flight_run`)
  is now folded into the long-poll request body's `status` entry
  (`design.md` §7.3, decision #7) — one entry per poll, since each
  poll is for one runner.
- The shared `out_tx`/`out_rx` mpsc for `Envelope<ClientMsg>` goes
  away. `RunnerOut::send` is rewired to call directly into that
  runner's `RunnerCloudClient`.
- The cross-runner **Demux** goes away entirely. With per-runner
  polls, each `HttpLoop`'s response stream is for exactly one
  runner — the response flows directly into that runner's mailbox
  with no fan-out routing.
- `hello_emitter` / `attach_emitter` / `HelloRunnerMap` /
  `AttachRunnerMap` go away. There is no separate attach step; a
  runner's session-open is its attach.
- The shared `mailboxes` / `status_sources` / `attach_runners`
  maps go away. Each `RunnerInstance` owns its own state.

## 2. New module — `runner/src/cloud/http.rs`

```
runner/src/cloud/
  mod.rs          // re-exports
  protocol.rs     // unchanged — ClientMsg/ServerMsg enums stay as JSON body schemas
  ws.rs           // legacy, kept for per-run WS upgrade (design.md §7.9)
  http.rs         // NEW — owns the HTTPS transport, per-runner clients
```

`http.rs` exposes three top-level types: `SharedHttpTransport`
(daemon-shared), `RunnerCloudClient` (per-runner auth + verbs),
`HttpLoop` (per-runner poll loop).

```rust
/// Daemon-shared transport pool. Cloned freely; the inner
/// reqwest::Client uses Arc internally so clones share the same
/// HTTP/1.1 keep-alive / HTTP/2 multiplex pool.
#[derive(Clone)]
pub struct SharedHttpTransport {
    http: reqwest::Client,        // HTTP/2 capable, keep-alive enabled
    cloud_url: String,
}

impl SharedHttpTransport {
    pub fn new(cloud_url: String) -> Result<Self> { /* ... */ }
}

/// One per RunnerInstance. Owns that runner's auth state machine,
/// session state, and refresh schedule. Cheap to clone — inner state
/// is Arc<Mutex<...>> so per-instance code (RunnerLoop, AssignWorker,
/// bridge handlers) can hold a clone and dispatch directly.
#[derive(Clone)]
pub struct RunnerCloudClient {
    runner_id: Uuid,
    creds: CredentialsHandle,            // per-runner refresh token + on-disk persistence
    inner: Arc<Mutex<RunnerCloudClientInner>>,
    transport: SharedHttpTransport,      // shared across all runners on this daemon
}

struct RunnerCloudClientInner {
    access_token: Option<AccessToken>,
    session: Option<SessionState>,       // session_id + server_time
                                         // (the cloud owns PEL-drained state per §8;
                                         // the daemon does not track it)
    refresh_in_flight: Option<RefreshInFlight>,  // single-flight guard, see §9
}

/// Materialised access token. The daemon does not verify the JWT —
/// it treats it as opaque per design.md §5.2 — but it parses `exp`
/// to schedule refreshes ~5 min before expiry.
struct AccessToken {
    raw: String,
    exp: DateTime<Utc>,
}

/// Per-runner long-poll loop. Spawned once per RunnerInstance by
/// Supervisor::run.
pub struct HttpLoop {
    client: RunnerCloudClient,            // owned by this loop
    state: StateHandle,                   // daemon-level; for shutdown signal
    runner_state: RunnerStateHandle,      // per-runner state for status snapshot
    mailbox: mpsc::Sender<Envelope<ServerMsg>>,  // direct to this runner's RunnerLoop
    ack_rx: mpsc::UnboundedReceiver<AckEntry>,   // from RunnerLoop after handle completes
}
```

`RunnerCloudClient` exposes one method per protocol verb (one-to-one
with `design.md` §7):

- `open_session(attach_metadata)` → `POST /runners/<rid>/sessions/`.
  Returns `SessionState` plus the `welcome` payload (and optional
  `resume_ack` if `in_flight_run` was set in the request body).
  Called by `HttpLoop` on startup and after **recoverable**
  transport errors. `409 session_evicted` is **not** recoverable —
  triggers shutdown of this `RunnerInstance` (sibling runners on
  the daemon continue running, per `design.md` §10).
- `close_session()` → `DELETE /runners/<rid>/sessions/<sid>/`.
  Called on shutdown.
- `poll(ack_list, status)` → `POST /runners/<rid>/sessions/<sid>/poll`.
  The hot path. Returns `Vec<PollResponseEntry>` to be dispatched.
- `post_run_event(run_id, batch)` → batched
  `POST /runs/<run_id>/events/`.
- `post_run_lifecycle(run_id, kind, body)` → POST to the appropriate
  per-run endpoint (`accept`, `started`, `complete`, `pause`, `fail`,
  `cancelled`, `resumed`, `awaiting-reauth`).
- `post_approval_request(run_id, body)` → `POST /runs/<run_id>/approvals/`.
- `refresh()` → `POST /runners/<rid>/refresh/`. Mints new refresh +
  access tokens, persists the refresh token to
  `runners/<rid>/credentials.toml` before discarding the old one in
  memory.
- `force_refresh_inline()` → like `refresh()`, but called from inside
  `HttpLoop` when a `force_refresh` server message arrives.

All POST methods set `Authorization: Bearer <access_token>` from
`RunnerCloudClientInner.access_token`. Methods that 401 with
`access_token_expired` automatically call `refresh()` (single-flight,
see §9) and retry once. A 401 with any other reason
(`membership_revoked`, `refresh_token_replayed`, `runner_revoked`,
`runner_id_mismatch`) propagates as a fatal error and triggers
shutdown of this `RunnerInstance` only — the daemon process and
sibling runners continue.

## 3. `RunnerOut` adapter

The trick that makes per-instance code unchanged: `RunnerOut::send`
keeps its existing signature but routes to the runner's own
`RunnerCloudClient`.

Today (`runner/src/daemon/runner_out.rs:39`):

```rust
pub async fn send(&self, body: ClientMsg) -> Result<(), SendError<...>> {
    self.inner.send(Envelope::for_runner(self.runner_id, body)).await
}
```

After Phase 4:

```rust
pub async fn send(&self, body: ClientMsg) -> Result<(), TransportError> {
    self.client.dispatch_client_msg(body).await
}
```

where `self.client: RunnerCloudClient` is the runner's own client
(the `runner_id` is implicit in the client). `dispatch_client_msg`
matches on the variant and calls the right HTTP method:

| `ClientMsg` variant | Action                                                                                       |
| ------------------- | -------------------------------------------------------------------------------------------- |
| `Hello`             | Disallowed at this seam — handled by `HttpLoop::run` at session-open (§4)                    |
| `Heartbeat`         | Disallowed at this seam — folded into poll body's `status` field (§5)                        |
| `Accept`            | `post_run_lifecycle(run_id, "accept", body)`                                                 |
| `RunStarted`        | `post_run_lifecycle(run_id, "started", body)`                                                |
| `RunEvent`          | `post_run_event(run_id, body)` (with batching, see §6)                                       |
| `ApprovalRequest`   | `post_approval_request(run_id, body)`                                                        |
| `RunAwaitingReauth` | `post_run_lifecycle(run_id, "awaiting-reauth", body)`                                        |
| `RunCompleted`      | `post_run_lifecycle(run_id, "complete", body)`                                               |
| `RunPaused`         | `post_run_lifecycle(run_id, "pause", body)`                                                  |
| `RunFailed`         | `post_run_lifecycle(run_id, "fail", body)`                                                   |
| `RunCancelled`      | `post_run_lifecycle(run_id, "cancelled", body)`                                              |
| `RunResumed`        | `post_run_lifecycle(run_id, "resumed", body)`                                                |
| `Bye`               | Disallowed at this seam — `RunnerInstance` shutdown calls `client.close_session()` directly. |

`RunnerOut::send_connection_scoped` becomes a hard-error: there are
no connection-scoped frames anymore in the per-runner architecture.
`Bye` was the only such frame and is now sent by the per-runner
shutdown path directly. Calling this method post-Phase-4 should
panic in debug / log-and-drop in release.

This means **every existing `out.send(ClientMsg::...)` call site in
`RunnerLoop`, `AssignWorker`, and the bridge handlers compiles
unchanged.**

## 4. Session-open flow (replaces `hello_emitter` / `attach_emitter`)

`HttpLoop::run` is where session-open happens. There is no separate
emitter task: a runner's session-open is the first thing its
`HttpLoop` does.

```rust
async fn run(mut self) -> Result<()> {
    // (1) Bootstrap auth: ensure we have a fresh access token.
    self.client.ensure_access_token().await?;

    // (2) Open the session. The body carries what today's per-runner
    //     Hello carries: version, os, arch, status, in_flight_run,
    //     project_slug, host_label, agent_versions.
    let attach_body = self.build_attach_body();
    self.client.open_session(attach_body).await?;

    // The cloud's session-open handler runs the existing _apply_hello
    // flow synchronously and returns Welcome (and optional ResumeAck).
    // Handle them inline: hand Welcome to the RunnerLoop's mailbox so
    // existing per-instance handlers ingest it as today.
    self.mailbox.send(welcome_envelope).await?;
    if let Some(resume) = resume_ack {
        self.mailbox.send(resume_ack_envelope).await?;
    }

    self.runner_state.set_connected(true).await;

    // (3) Poll forever.
    loop {
        tokio::select! {
            _ = self.state.shutdown_notified().notified() => break,
            result = self.poll_once() => match result {
                Ok(_) => continue,
                Err(e) if is_recoverable(&e) => {
                    backoff().await;
                    self.try_reopen_session().await;
                }
                Err(e) => return Err(e),  // fatal — RunnerInstance shuts down
            }
        }
    }

    self.client.close_session().await.ok();
    Ok(())
}
```

`try_reopen_session` reuses `open_session` after a network blip /
5xx. A `409 session_evicted` is **not** recoverable here — that's
the displacing daemon's signal that we no longer own this runner;
the loop returns with a fatal error and the `RunnerInstance` shuts
down. Sibling `RunnerInstance`s in the same daemon continue
running.

## 5. Heartbeat / status folding

Today, per-runner heartbeat tasks (`supervisor.rs:251`, `hb_handles`)
fire on a timer and emit `ClientMsg::Heartbeat` envelopes. With the
new transport, `Heartbeat` no longer exists as a discrete frame —
its fields live in the long-poll request body's `status` entry
(`design.md` §7.3) — **one entry per poll, since each poll is for
one runner**.

The mechanic:

- Each `RunnerInstance` exposes a `RunnerStateHandle` (or equivalent)
  that gives the `HttpLoop` cheap watch-channel reads of `status`
  and `in_flight_run`.
- `HttpLoop::poll_once` snapshots that runner's status immediately
  before sending the poll, builds a single-entry `status` field on
  the request body.
- The `ts` field is the moment of snapshot, not the moment a state
  last changed — fine; the cloud's `_reap_stale_busy_runs` uses it
  as a recency floor.
- The `hb_handles` Vec, `hb_join`, and the per-instance interval
  ticker logic all delete.

Liveness threshold (`runner_offline_threshold_secs = 50` in
`design.md` §9) is unchanged from today's heartbeat-based offline
detection, so no operational tuning is needed.

## 6. `RunEvent` batching

`design.md` decision #11 specifies a 250 ms / 64 KB batch trigger
for `RunEvent` POSTs. This lives inside `RunnerCloudClient` and is
opaque to `RunnerOut`'s callers:

- Each `dispatch_client_msg(ClientMsg::RunEvent { run_id, .. })`
  appends to a per-run buffer keyed by `run_id`.
- A timer (250 ms) and a size threshold (64 KB serialized) fire
  whichever comes first; the buffer is flushed via a single
  `post_run_event(run_id, batch)`.
- On `RunnerInstance` shutdown, all buffers are drained synchronously
  before `close_session()`.

The buffer is per-run and per-runner (each `RunnerCloudClient` has
its own); concurrent runs on different runners get fully
independent buffers and POSTs.

Lifecycle ordering contract:

- for a given `run_id`, `RunnerCloudClient` serializes non-event
  lifecycle POSTs and awaits each before sending the next lifecycle
  POST for that same run
- different runs may POST concurrently
- `RunEvent` batching remains independent, so cloud handlers must
  tolerate `RunEvent` arriving before `RunStarted`

## 7. No Demux

The cross-runner Demux from the old design is gone entirely. Each
`HttpLoop`'s response stream is for exactly one runner: there is
nothing to fan out.

`HttpLoop::dispatch_response` becomes a thin dispatcher that maps
each `PollResponseEntry` → `Envelope<ServerMsg>` → the runner's
mailbox. Connection-scoped messages don't exist in the per-runner
protocol; the entries that used to be connection-scoped (`Revoke`,
`force_refresh`) are now per-runner and ride the same per-runner
stream.

```rust
async fn dispatch_response(&self, msgs: Vec<PollResponseEntry>) {
    for entry in msgs {
        let env = Envelope::<ServerMsg>::from_entry(&entry);
        match &env.body {
            // ForceRefresh handled inline — never propagated to the
            // RunnerLoop, since it's a transport-level concern.
            ServerMsg::ForceRefresh { .. } => {
                if let Err(e) = self.client.force_refresh_inline().await {
                    tracing::error!(runner_id = %self.client.runner_id, "force_refresh failed: {e:#}");
                }
                let _ = self.ack_tx.send(AckEntry { stream_id: env.stream_id.clone() });
            }
            // Revoke handled inline — triggers RunnerInstance shutdown.
            ServerMsg::Revoke { .. } => {
                self.runner_state.shutdown().await;
                let _ = self.ack_tx.send(AckEntry { stream_id: env.stream_id.clone() });
                return;  // no further messages from this poll
            }
            // Everything else goes to the RunnerLoop's mailbox; ack
            // is sent by RunnerLoop after the handler completes (§8).
            _ => {
                let _ = self.mailbox.send(env).await;
            }
        }
    }
}
```

`RunnerLoop` mailbox shape and per-instance handlers (`Welcome`,
`Assign`, `Cancel`, `Decide`, `ConfigPush`, `RemoveRunner`,
`ResumeAck`) are unchanged.

## 8. Acks (ack-on-handle, per design.md decision #21)

Acks use the **explicit flat list** form `["<stream_id>", ...]`. They
go in the next poll's request body and translate server-side to
`XACK runner_stream:{rid} runner-group:{rid} <id1> [<id2> ...]`
(`design.md` §7.4).

**Ack-on-handle**: a stream id enters `HttpLoop`'s ack queue only
after the runner's handler in `RunnerLoop` has finished processing
the message — _not_ when `HttpLoop` dispatched it into the mailbox.
At-least-once delivery; redelivery is handled by per-instance
`InboundDedupe`.

Plumbing:

```rust
// Created per RunnerInstance:
let (ack_tx, ack_rx) = mpsc::unbounded_channel::<AckEntry>();

struct AckEntry {
    stream_id: String,
}

// HttpLoop owns ack_rx and drains on each poll cycle:
fn drain_pending_acks(&mut self) -> Vec<String> {
    let mut acks = Vec::new();
    while let Ok(entry) = self.ack_rx.try_recv() {
        acks.push(entry.stream_id);
    }
    acks
}

// RunnerLoop holds a clone of ack_tx; after each handler returns:
async fn handle_one(&mut self, env: Envelope<ServerMsg>) {
    let stream_id = env.stream_id.clone();
    if self.inbound_dedupe.seen(&env.message_id) {
        // Re-delivery from PEL after a prior crash. Ack and skip.
        let _ = self.ack_tx.send(AckEntry { stream_id });
        return;
    }
    self.dispatch(env.body).await;
    self.inbound_dedupe.record(env.message_id);
    let _ = self.ack_tx.send(AckEntry { stream_id });
}
```

`InboundDedupe` is a small bounded LRU (capacity ~256, optional TTL
~5 min) keyed on `Envelope.message_id`. Per-instance, not shared
across runners.

`HttpLoop`-handled inline messages (`ForceRefresh`, `Revoke`) ack
directly via `ack_tx` after their inline handler completes (§7).

**Session first poll uses `0`**, subsequent polls use `>` (per
`design.md` §7.3 step 5). The cloud tracks `session_pel_drained:{sid}`
so the daemon doesn't need to track this client-side.

## 9. Refresh scheduling and single-flight

A separate `refresh_loop` task per `RunnerInstance` (one alongside
each `HttpLoop`):

```rust
async fn refresh_loop(
    client: RunnerCloudClient,
    state: StateHandle,
) {
    loop {
        let exp = client.access_token_exp().await;
        let now = Utc::now();
        let safety = Duration::from_secs(300);
        let sleep_for = exp.signed_duration_since(now) - safety.into();
        tokio::select! {
            _ = state.shutdown_notified().notified() => return,
            _ = tokio::time::sleep(sleep_for.to_std().unwrap_or_default()) => {}
        }
        if let Err(e) = client.refresh().await {
            tracing::error!(runner_id = %client.runner_id, "scheduled refresh failed: {e:#}");
        }
    }
}
```

`force_refresh_inline()` from `HttpLoop` and the auto-retry on `401
access_token_expired` from any POST also call `client.refresh()`.

**Single-flight refresh** — required because multiple paths can call
`refresh()` concurrently within one runner: the scheduled refresh
loop, an inline `force_refresh`, and a 401 retry on a parallel
`post_run_event`. Implementation:

```rust
struct RefreshInFlight {
    waiter: tokio::sync::watch::Receiver<Option<Result<(), TransportError>>>,
}

impl RunnerCloudClient {
    pub async fn refresh(&self) -> Result<(), TransportError> {
        // Fast path: if a refresh is already in flight, await its result.
        {
            let inner = self.inner.lock().await;
            if let Some(rif) = &inner.refresh_in_flight {
                let mut rx = rif.waiter.clone();
                drop(inner);
                rx.changed().await.ok();
                return rx.borrow().clone().unwrap_or(Ok(()));
            }
        }
        // Slow path: install a waiter, perform the refresh, broadcast.
        let (tx, rx) = tokio::sync::watch::channel(None);
        {
            let mut inner = self.inner.lock().await;
            inner.refresh_in_flight = Some(RefreshInFlight { waiter: rx });
        }
        let result = self.do_refresh().await;
        {
            let mut inner = self.inner.lock().await;
            inner.refresh_in_flight = None;
        }
        let _ = tx.send(Some(result.clone()));
        result
    }
}
```

Cross-runner refreshes are independent (different
`RunnerCloudClient` instances, different tokens) — no coordination
needed.

## 10. Updated `Supervisor::run` skeleton

Annotated diff against `runner/src/daemon/supervisor.rs:56`:

```text
pub async fn run(self) -> Result<()> {
    // (1) workspace conflict checks — UNCHANGED.

    // (2) Build N RunnerInstances — UNCHANGED in shape, but each
    //     RunnerInstance now also carries:
    //       - its own per-runner credentials handle (reads from
    //         ~/.config/apple-pi-dash-runner/runners/<rid>/credentials.toml)
    //       - its own RunnerCloudClient
    //       - its own (ack_tx, ack_rx) mpsc pair
    //     RunnerInstance::new takes a SharedHttpTransport clone.

    // (3) IPC handle — UNCHANGED.

    // (4) Build the SharedHttpTransport once. All runners share it.
    let transport = SharedHttpTransport::new(cloud_url)?;

    // (5) Spawn per-runner task trees. No shared cloud_handle, no
    //     shared connected Notify, no shared mailbox/status maps.
    let mut runner_handles = Vec::new();
    for inst in instances {
        // Each RunnerInstance gets its own:
        //   - HttpLoop (poll loop + session lifecycle + dispatch)
        //   - refresh_loop (scheduled refresh)
        //   - RunnerLoop (existing per-runner agent driver)
        let http_handle = tokio::spawn(inst.http_loop().run());
        let refresh_handle = tokio::spawn(refresh_loop(
            inst.client.clone(),
            state.clone(),
        ));
        let runner_handle = tokio::spawn(inst.runner_loop().run());
        runner_handles.push((inst.runner_id, http_handle, refresh_handle, runner_handle));
    }

    // (6) Wait on shutdown signal — UNCHANGED.

    // (7) Per-runner clean shutdown: each HttpLoop sends Bye via
    //     client.close_session() before its task ends. The
    //     supervisor's shutdown signal triggers this through each
    //     HttpLoop's own select! arm.

    // (8) Join / abort all per-runner task trees.
    for (_, http, refresh, runner) in runner_handles {
        http.abort();
        refresh.abort();
        runner.abort();
    }
    ipc_handle.abort();
    Ok(())
}
```

## 11. Error / failure model

Per `design.md` §10, mapped to daemon-side response. **All errors
are scoped to one `RunnerInstance`** — the daemon process and
sibling runners are unaffected unless the error is a daemon-level
concern (e.g. shared transport pool failure, but those are
recoverable).

| Wire response                      | `RunnerCloudClient` action                                                                                                                                                                                                                                                                                | Visible to `RunnerLoop`?                  |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| 401 `access_token_expired`         | call `refresh()` (single-flight), retry once. Transparent.                                                                                                                                                                                                                                                | No.                                       |
| 401 `runner_id_mismatch`           | Programmer error (URL `<rid>` mismatched the token's `sub`). Log + shutdown this `RunnerInstance`.                                                                                                                                                                                                        | Mailbox closes; this runner's loops exit. |
| 401 `membership_revoked`           | This runner's owning user lost workspace access. `runner_state.shutdown()`. This `RunnerInstance` dies; sibling runners continue.                                                                                                                                                                         | Same.                                     |
| 401 `refresh_token_replayed`       | This runner's refresh token was replayed. `runner_state.shutdown()`.                                                                                                                                                                                                                                      | Same.                                     |
| 401 `runner_revoked`               | This runner was revoked server-side. `runner_state.shutdown()`.                                                                                                                                                                                                                                           | Same.                                     |
| 409 `session_evicted` on poll      | Another daemon owns this runner now. `runner_state.shutdown()`.                                                                                                                                                                                                                                           | Same.                                     |
| 409 `session_evicted` on lifecycle | Same — fatal for this `RunnerInstance`.                                                                                                                                                                                                                                                                   | Same.                                     |
| 409 `concurrent_poll`              | Internal logic error. Log + retry once after 100 ms. Persistent → shutdown this `RunnerInstance`.                                                                                                                                                                                                         | Initially no; alert ops on pattern.       |
| 429 `poll_rate_exceeded`           | Backoff to ≥5 s between polls. Recovers.                                                                                                                                                                                                                                                                  | No.                                       |
| Network error / 5xx                | Exponential backoff ≤30 s. Re-poll. If session is stale on recovery, `open_session()` gets a fresh one.                                                                                                                                                                                                   | Brief silence; loops continue.            |
| `force_refresh` ServerMsg          | `force_refresh_inline()`; do not surface to `RunnerLoop`.                                                                                                                                                                                                                                                 | No.                                       |
| `Revoke` ServerMsg                 | Inline: `runner_state.shutdown()`. Ack the stream id.                                                                                                                                                                                                                                                     | Mailbox closes; this runner's loops exit. |
| `RemoveRunner` ServerMsg           | Demux to mailbox. `RunnerLoop` exits its inner loop on this frame (same as today). On exit, the runner's `HttpLoop` calls `client.close_session()` and the supervisor reaps the task tree. The other runners on the daemon keep working.                                                                  | Yes — same as today.                      |
| Daemon crash mid-handler           | Process killed before `RunnerLoop` could send the ack. Stream id stays in `consumer-{sid}`'s PEL on `runner_stream:{rid}`. New session-open `XAUTOCLAIM`s onto `consumer-{new_sid}` (paginated); first poll `XREADGROUP ... 0` redelivers; `InboundDedupe` lets the handler skip if it had partially run. | Yes (re-delivery).                        |

Runner shutdown with in-flight agent subprocess:

- if a `RunnerInstance` shuts down while an agent subprocess is still
  running, shutdown order is:
  1. signal the agent subprocess to stop (best-effort graceful stop,
     escalate after a short grace period)
  2. if shutdown is a normal daemon-side shutdown and auth is still
     valid, send a final `RunCancelled` POST with
     `reason="runner_shutdown"`
  3. if shutdown was triggered by `runner_revoked`,
     `membership_revoked`, or `refresh_token_replayed`, skip the final
     lifecycle POST because the cloud-side `Runner.revoke()` cascade
     has already cancelled the run
  4. stop the `RunnerLoop`

## 12. Module-level test plan

- **`http.rs::SharedHttpTransport` unit tests**: HTTP/2 negotiation;
  cloning preserves the shared connection pool.
- **`http.rs::RunnerCloudClient` unit tests**: per-runner refresh
  state machine (`expired → refresh → retry`); single-flight refresh
  under concurrent callers; refresh failure modes (`replayed`,
  `membership_revoked`, `runner_revoked`); session open/close.
- **Single-flight refresh stress**: force 10 concurrent callers on one
  `RunnerCloudClient` to hit `401 access_token_expired`
  simultaneously; assert exactly one refresh occurs and all callers
  retry with the new token.
- **`HttpLoop` unit tests**: ack-on-handle accumulation; ack drain
  happens at next poll; `force_refresh` triggers inline refresh;
  `Revoke` triggers `RunnerInstance` shutdown; `concurrent_poll`
  retry behavior.
- **`InboundDedupe` unit tests**: insert-and-seen returns true for
  same `mid`; LRU evicts oldest when full; distinct `mid` values
  pass through.
- **End-to-end ack-on-handle**: dispatch a poll containing 3
  messages for one runner; assert that the next poll's `ack` list
  only includes ids whose handlers have completed (block one
  handler in the test and confirm its id is absent).
- **`RunnerOut::send` dispatch table**: one test per `ClientMsg`
  variant. Effectively a `match`-completeness test.
- **Multi-runner isolation**: drive two `RunnerInstance`s in one
  daemon against a fake cloud. Inject a 5xx storm on runner A's
  poll; assert runner B's poll/refresh/event POSTs continue
  unaffected. Inject `409 session_evicted` for runner A; assert
  runner A shuts down cleanly while runner B continues.
- **Integration (against fake cloud)**: end-to-end Assign → Accept
  → RunEvent×N → RunCompleted; `force_refresh` mid-run for one
  runner; session eviction (`409`) for one runner; cloud restart
  drops a poll, runner recovers via re-poll.

## 13. Phase 4 sub-phases

The work decomposes naturally; each sub-phase is independently
mergeable behind a runtime flag (`PI_DASH_TRANSPORT=http|ws`) until
4d ships:

- **4a** — scaffold `runner/src/cloud/http.rs`. `SharedHttpTransport`,
  `RunnerCloudClient` with `refresh()`, `open_session()`,
  `close_session()`. Single-flight refresh implementation. Per-runner
  credentials handle reading from
  `~/.config/apple-pi-dash-runner/runners/<rid>/credentials.toml`.
  Single integration test against a fake cloud. No supervisor
  changes; daemon still uses WS in this commit.
- **4b** — replace `ConnectionLoop` with per-runner `HttpLoop` in
  `Supervisor::run`. Remove the shared `out_tx`/`out_rx` mpsc.
  Adapt `RunnerOut::send` to dispatch via the runner's
  `RunnerCloudClient`. Drop the standalone Demux task and the
  shared mailbox/status_sources/attach_runners maps. Drop
  `hello_emitter`. End-to-end integration test (one runner) passes.
- **4c** — fold heartbeat into per-runner poll body's `status` field.
  Drop `hb_handles`. Per-runner liveness assertions in tests.
  Multi-runner isolation test added.
- **4d** — `force_refresh` handler. Per-runner `refresh_loop` task.
  Bump `WIRE_VERSION`/`PROTOCOL_VERSION` to 4. Default
  `PI_DASH_TRANSPORT=http`.

(Note: the old design had a separate Phase 4d for `attach_emitter`
replacement; that step is gone in the per-runner architecture
because session-open is inline in `HttpLoop::run`.)

## 14. Open daemon-side questions

- **Worktree / per-run resource cleanup on `409 session_evicted`**:
  the runner shuts down without sending lifecycle frames for any
  in-flight run. The cloud-side `Runner.revoke()` cascade (which is
  _not_ called on simple session-eviction) doesn't run, so
  in-flight `AgentRun` rows stay `RUNNING` until heartbeat-staleness
  reaping fires. Acceptable in v1; document the latency.
- **Mailbox backpressure under load**: today's mailbox is bounded
  `mpsc(...)` (capacity not yet specified). If the channel fills,
  `tx.send().await` parks the dispatching `HttpLoop`, which delays
  acks for that runner only — sibling runners unaffected. Worth
  measuring under load before tuning.
- **HTTP/2 vs HTTP/1.1**: `reqwest` negotiates automatically. HTTP/2
  is preferable for keep-alive multiplexing of the many concurrent
  POSTs across N runners on one daemon. Pin to
  `reqwest = { features = ["http2"] }` and verify in integration
  tests.
- **Single-flight refresh tuning**: the `tokio::sync::watch`-based
  implementation in §9 broadcasts the result to all waiters. If
  N concurrent callers blow past the watch buffer, fall back to a
  `OnceCell`-style implementation. Measure first.

## 15. Out of scope for the daemon-side phase 4

- Runner workers as subprocesses. The per-runner credentials file
  layout (§5.5) makes this a future configuration change rather
  than a refactor, but it's not built in v1.
- TUI redesign beyond the new per-runner `polling` / `refreshing` /
  `session_evicted` status indicators. The IPC shape
  (`StatusSnapshot`) does not change.
- `runner/src/cli/`-level changes for `pidash connect --count N`.
- Changes to the agent bridge (`runner/src/codex/`,
  `runner/src/claude_code/`). Bridges interact with `RunnerOut` via
  the existing call-site shape and are unaffected by transport
  changes.
