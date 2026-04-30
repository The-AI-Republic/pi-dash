# Daemon-Side Module Design — HTTPS Long-Poll Transport

> Companion to `.ai_design/move_to_https/design.md`. That doc
> specifies the wire protocol and cloud-side implementation. This doc
> specifies how the new transport plugs into the existing daemon
> architecture from `.ai_design/n_runners_in_same_machine/`.
>
> Concretely: how `runner/src/cloud/http.rs` (new) replaces
> `runner/src/cloud/ws.rs`'s role in `Supervisor::run` while preserving
> the per-`RunnerInstance` mailboxes, `RunnerOut` semantics, the
> hello-on-reconnect pattern, and the demux fan-out that
> `runner/src/daemon/supervisor.rs` already orchestrates.

## 1. What stays, what changes

The daemon's existing decomposition is sound and is **kept** in full:

- `Supervisor` (`runner/src/daemon/supervisor.rs`) still owns the
  lifecycle of N `RunnerInstance`s, the mailbox map, the
  hello-emitter pattern, and IPC.
- `RunnerInstance` (`runner/src/daemon/runner_instance.rs`) is
  unchanged; it continues to own per-runner state (`StateHandle`,
  `ApprovalRouter`, mailbox sender/receiver pair, paths, config).
- `RunnerLoop` (`supervisor.rs:431`) — per-instance task that
  consumes from its mailbox, drives the agent bridge, posts upstream
  events — is unchanged in shape. Its inputs and outputs are
  reshaped, but the loop body is the same.
- `RunnerOut` (`runner/src/daemon/runner_out.rs`) — the per-instance
  send wrapper that callers in `RunnerLoop`/`AssignWorker` use — is
  unchanged at the call site. Its **internal dispatch** changes (see
  §3 below). This is the central insight that makes the migration
  cheap: every call site that today does `out.send(ClientMsg::...)`
  keeps working.
- `Demux` — the routing logic at `supervisor.rs:268-312` that maps
  inbound `Envelope<ServerMsg>` → per-instance mailbox by
  `runner_id` — is **kept**, but its input source moves from a
  WS-fed mpsc to the long-poll response.
- `hello_emitter` — the watcher at `supervisor.rs:401-429` that
  re-fires a per-runner `Hello` on every fresh handshake — is kept
  in shape but emits **`POST .../attach/`** instead of a `Hello`
  envelope. The `connected` Notify mechanism stays exactly as is.

What goes away:

- `runner/src/cloud/ws.rs::ConnectionLoop` is no longer dialed from
  `Supervisor::run`. The `ws.rs` module stays in the build for the
  per-run upgrade ticket path described in `design.md` §7.9.
- The dedicated per-runner heartbeat tasks (`hb_handles` in
  `supervisor.rs:251`) go away. Status (`status`, `in_flight_run`)
  is now folded into the long-poll request body's `status[]` vector
  (`design.md` §7.3, decision #7).
- The `out_tx`/`out_rx` mpsc channel for `Envelope<ClientMsg>` goes
  away as a single funnel. `RunnerOut::send` is rewired to call
  directly into the new `HttpClient` (§3).

## 2. New module — `runner/src/cloud/http.rs`

```
runner/src/cloud/
  mod.rs          // re-exports
  protocol.rs     // unchanged — ClientMsg/ServerMsg enums stay as JSON body schemas
  ws.rs           // legacy, kept for per-run WS upgrade (design.md §7.9)
  http.rs         // NEW — owns the HTTPS transport
```

`http.rs` exposes three top-level types:

```rust
/// Owns the access token, refresh credential, and the active session
/// (if any). One per daemon. Cheap to clone — the inner state is in
/// an `Arc<Mutex<...>>` so per-instance code can hold a clone and
/// call directly.
pub struct HttpClient {
    creds: CredentialsHandle,            // refresh_token + on-disk persistence
    inner: Arc<Mutex<HttpClientInner>>,  // access_token + session state
    cloud_url: String,
    http: reqwest::Client,               // keep-alive, HTTP/2 capable
}

/// State that mutates over the daemon's lifetime.
struct HttpClientInner {
    access_token: Option<AccessToken>,   // self-contained, in-memory only
    session: Option<SessionState>,       // session_id + server_time + ack cursors
}

/// Long-poll loop. Spawned once by `Supervisor::run`. Replaces the
/// per-WS `ConnectionLoop::run`.
pub struct HttpLoop {
    client: HttpClient,
    state: StateHandle,                  // daemon-level; for connected/shutdown
    /// Demux fan-out. Same `mailboxes` map the supervisor populates
    /// today on each `RunnerInstance` attach. `HttpLoop` reads the
    /// long-poll response and dispatches into these.
    mailboxes: Arc<RwLock<HashMap<Uuid, mpsc::Sender<Envelope<ServerMsg>>>>>,
    /// Per-runner status snapshot. `HttpLoop` reads from these to
    /// build the `status[]` vector on every poll request.
    status_sources: Arc<RwLock<HashMap<Uuid, RunnerStatusSource>>>,
    /// Fired on every fresh-session establishment. Drives the
    /// `attach_emitter` to re-attach all runners. Same Notify
    /// shape as today's `connected`.
    connected: Arc<tokio::sync::Notify>,
}

/// Materialised access token (JWT body extracted to drive refresh
/// timing). The daemon does not verify the JWT — it treats it as
/// opaque per design.md §5.2 — but it does parse `exp` so it can
/// schedule refreshes ~5 min before expiry.
struct AccessToken {
    raw: String,
    exp: DateTime<Utc>,
}
```

`HttpClient` exposes one method per protocol verb (one-to-one with
`design.md` §7):

- `open_session()` → `POST /sessions/`. Returns `SessionState`. Called
  by `HttpLoop` on startup and after **recoverable** transport errors
  (network blip, 5xx). `409 session_evicted` is **not** recoverable —
  it triggers daemon shutdown per `design.md` §10 and §11 below.
- `attach_runner(runner_id, attach_body)` → `POST .../attach/`.
  Idempotent on the cloud side; the daemon may retry on transient
  errors.
- `detach_runner(runner_id)` → `DELETE .../runners/<rid>/`. Called
  on `RemoveRunner` server message and on shutdown.
- `poll(ack_map, status_vec)` → `POST .../poll`. The hot path.
  Returns `Vec<Envelope<ServerMsg>>` to be demuxed.
- `post_run_event(run_id, batch)` → batched `POST /runs/<run_id>/events/`.
- `post_run_lifecycle(run_id, kind, body)` → POST to the appropriate
  per-run endpoint (`accept`, `started`, `complete`, `pause`, `fail`,
  `cancelled`, `resumed`, `awaiting-reauth`).
- `post_approval_request(run_id, body)` → `POST /runs/<run_id>/approvals/`.
- `refresh()` → `POST /connections/<cid>/refresh/`. Mints a new
  refresh + access token, persists the refresh token to the on-disk
  credentials file before discarding the old one in memory.
- `force_refresh_inline()` → like `refresh()`, but called from inside
  the poll loop when a `force_refresh` server message arrives.

All POST methods set `Authorization: Bearer <access_token>` from
`HttpClientInner.access_token`. Methods that 401 with
`access_token_expired` automatically call `refresh()` and retry once.
A 401 with any other reason (`membership_revoked`,
`refresh_token_replayed`) propagates as a fatal error and triggers
daemon shutdown via `state.shutdown()`.

## 3. `RunnerOut` adapter

The trick that makes per-instance code unchanged: `RunnerOut::send`
keeps its existing signature but routes by `ClientMsg` variant.

Today (`runner/src/daemon/runner_out.rs:39`):

```rust
pub async fn send(&self, body: ClientMsg) -> Result<(), SendError<...>> {
    self.inner.send(Envelope::for_runner(self.runner_id, body)).await
}
```

After Phase 4:

```rust
pub async fn send(&self, body: ClientMsg) -> Result<(), TransportError> {
    self.client.dispatch_client_msg(self.runner_id, body).await
}
```

where `HttpClient::dispatch_client_msg` matches on the variant and
calls the right HTTP method:

| `ClientMsg` variant | Action                                                                                                  |
| ------------------- | ------------------------------------------------------------------------------------------------------- |
| `Hello`             | Disallowed at this seam — handled by `attach_emitter` (§4)                                              |
| `Heartbeat`         | Disallowed at this seam — folded into poll body (§5)                                                    |
| `Accept`            | `post_run_lifecycle(run_id, "accept", body)`                                                            |
| `RunStarted`        | `post_run_lifecycle(run_id, "started", body)`                                                           |
| `RunEvent`          | `post_run_event(run_id, body)` (with batching, see §6)                                                  |
| `ApprovalRequest`   | `post_approval_request(run_id, body)`                                                                   |
| `RunAwaitingReauth` | `post_run_lifecycle(run_id, "awaiting-reauth", body)`                                                   |
| `RunCompleted`      | `post_run_lifecycle(run_id, "complete", body)`                                                          |
| `RunPaused`         | `post_run_lifecycle(run_id, "pause", body)`                                                             |
| `RunFailed`         | `post_run_lifecycle(run_id, "fail", body)`                                                              |
| `RunCancelled`      | `post_run_lifecycle(run_id, "cancelled", body)`                                                         |
| `RunResumed`        | `post_run_lifecycle(run_id, "resumed", body)`                                                           |
| `Bye`               | Disallowed at this seam — supervisor calls `client.close_session()` directly on shutdown (§10 step 12). |

`RunnerOut::send_connection_scoped` becomes a hard-error: `Bye` was
the only connection-scoped frame and is now sent by the supervisor's
shutdown path directly. Calling this method post-Phase-4 is a
programmer error and should panic in debug / log-and-drop in
release. If future connection-scoped events appear, they get a
matching method on `HttpClient`.

This means **every existing `out.send(ClientMsg::...)` call site in
`RunnerLoop`, `AssignWorker`, and the bridge handlers compiles
unchanged.**

## 4. `attach_emitter` (replaces `hello_emitter`)

`hello_emitter` (`supervisor.rs:401`) currently:

1. `connected.notified().await` — waits for fresh handshake.
2. Reads the `hello_runners` map.
3. For each runner, builds and sends a `ClientMsg::Hello` via
   `RunnerOut`.

`attach_emitter` is the same shape, replacing step 3:

```rust
async fn attach_emitter(
    runners: Arc<RwLock<AttachRunnerMap>>,
    connected: Arc<tokio::sync::Notify>,
    daemon_state: StateHandle,
    client: HttpClient,
) {
    loop {
        connected.notified().await;
        daemon_state.set_connected(true).await;
        let current: Vec<AttachRunner> =
            { runners.read().await.values().cloned().collect() };
        for r in current {
            let body = AttachRequest {
                version: crate::RUNNER_VERSION.into(),
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
                status: *r.state.rx_status.borrow(),
                in_flight_run: *r.state.rx_in_flight.borrow(),
                project_slug: r.project_slug.clone(),
            };
            // Cloud-side `attach/` is idempotent; retrying after
            // session-eviction recovery is safe.
            let _ = client.attach_runner(r.runner_id, body).await;
        }
    }
}
```

`HttpLoop` fires `connected.notify_one()`:

- After a successful `open_session()` on cold start.
- After a successful `open_session()` triggered by recovery
  (`409 session_evicted` causes shutdown by spec — but in the
  reconnect-on-network-error path, a fresh session-open also fires
  `connected`).

The `connected` Notify pattern is unchanged from today. Only the
emit-side payload changes.

## 5. Heartbeat / status folding

Today, per-runner heartbeat tasks (`supervisor.rs:251`, `hb_handles`)
fire on a timer and emit `ClientMsg::Heartbeat` envelopes via
`out_tx`. With the new transport, `Heartbeat` no longer exists as a
discrete frame — its fields live in the long-poll request body's
`status[]` vector (`design.md` §7.3).

The mechanic:

- `Supervisor::run` registers each `RunnerInstance` in a
  `status_sources: Arc<RwLock<HashMap<Uuid, RunnerStatusSource>>>`
  map. `RunnerStatusSource` is a thin handle that exposes
  `(status, in_flight_run, last_updated_ts)` as cheap watch-channel
  reads — `RunnerInstance` already has `state.rx_status` and
  `state.rx_in_flight` watch receivers. Wrap them in
  `RunnerStatusSource` and stash a clone.
- `HttpLoop::poll_once` builds `status[]` by snapshotting every
  entry in `status_sources` immediately before sending the poll.
- The `ts` field is the moment of snapshot, not the moment a state
  last changed — this is fine; `_reap_stale_busy_runs` on the cloud
  uses it as a recency floor.
- The `hb_handles` Vec, `hb_join`, and the per-instance interval
  ticker logic all delete.

Liveness threshold (`runner_offline_threshold_secs = 50` in §9) is
unchanged from today's heartbeat-based offline detection, so no
operational tuning is needed.

## 6. `RunEvent` batching

`design.md` decision #11 specifies a 250 ms / 64 KB batch trigger
for `RunEvent` POSTs. This lives inside `HttpClient` and is opaque
to `RunnerOut`'s callers:

- Each `dispatch_client_msg(_, ClientMsg::RunEvent { run_id, .. })`
  appends to a per-run buffer keyed by `run_id`.
- A timer (250 ms) and a size threshold (64 KB serialized) fire
  whichever comes first; the buffer is flushed via a single
  `post_run_event(run_id, batch)`.
- On daemon shutdown, all buffers are drained synchronously before
  `Bye`.

The buffer is per-run, not per-runner, because batches share an
endpoint URL keyed on `run_id`. Multiple concurrent runs on
different runners get independent buffers.

## 7. Demux

The Demux logic at `supervisor.rs:268-312` is moved into
`HttpLoop::dispatch_response`:

```rust
async fn dispatch_response(&self, msgs: Vec<PollResponseEntry>) {
    for entry in msgs {
        let env = Envelope::<ServerMsg>::from_entry(&entry);
        match env.runner_id {
            Some(rid) => {
                let mb = { self.mailboxes.read().await.get(&rid).cloned() };
                if let Some(tx) = mb {
                    let _ = tx.send(env).await;
                } else {
                    tracing::warn!(%rid, "frame for unknown runner; dropping");
                }
            }
            None => self.handle_connection_scoped(env).await,
        }
    }
}
```

Connection-scoped messages handled inline (matches today's
behavior, per `supervisor.rs:284-309`):

- `ServerMsg::Revoke` → `state.shutdown()`.
- `ServerMsg::Ping` → no-op (long-poll itself replaces it; if the
  cloud emits a Ping anyway during phase-4 dual-stack, ignore).
- New: `ServerMsg::ForceRefresh` → `client.force_refresh_inline()`,
  do not propagate to instances.

`RunnerLoop` mailbox shape and `Demux`-to-mailbox dispatch are
unchanged, so per-instance handlers (`Welcome`, `Assign`, `Cancel`,
`Decide`, `ConfigPush`, `RemoveRunner`, `ResumeAck`) all keep
working without modification.

## 8. Acks (ack-on-handle, per design.md decision #21)

Acks are per-runner and use the **explicit list** form
`{runner_id: [stream_id, ...]}`. They go in the next poll's request
body and translate server-side to
`XACK runner_stream:{rid} runner-group:{rid} <id1> [<id2> ...]`
(XACK takes exact ids, not a range — `design.md` §7.4).

**Ack-on-handle**: a stream id enters `HttpLoop`'s `ack_map` only
after the per-runner handler in `RunnerLoop` has finished processing
the message — _not_ the moment `HttpLoop` dispatched it into the
mailbox. This is at-least-once delivery; redelivery is handled by
per-instance dedupe (below).

Plumbing:

```rust
// Created per RunnerInstance and shared into HttpLoop:
let (ack_tx, ack_rx) = mpsc::unbounded_channel::<AckEntry>();

struct AckEntry {
    runner_id: Uuid,
    stream_id: String,
}

// HttpLoop owns the receivers and drains them on each poll cycle:
fn drain_pending_acks(&self) -> AckMap {
    let mut acks: AckMap = HashMap::new();
    while let Ok(entry) = self.ack_rx.try_recv() {
        acks.entry(entry.runner_id).or_default().push(entry.stream_id);
    }
    acks
}

// RunnerLoop: after the inner handler returns successfully:
async fn handle_one(&mut self, env: Envelope<ServerMsg>) {
    let stream_id = env.stream_id.clone();
    if self.inbound_dedupe.seen(&env.message_id) {
        // Re-delivery from PEL after a prior crash. Ack and skip.
        let _ = self.ack_tx.send(AckEntry { runner_id: self.runner_id, stream_id });
        return;
    }
    self.dispatch(env.body).await;
    self.inbound_dedupe.record(env.message_id);
    let _ = self.ack_tx.send(AckEntry { runner_id: self.runner_id, stream_id });
}
```

`InboundDedupe` is a small bounded LRU (capacity ~256, optional TTL
~5 min) keyed on `Envelope.message_id`. Sized to comfortably exceed
any realistic in-flight window between fetch and handler completion.
Per-instance, not shared across runners — distinct runners cannot
re-deliver each other's messages.

**Connection-scoped messages** handled inline in `HttpLoop` (Revoke,
ForceRefresh, etc.) ack via the same channel — they are also
ack-on-handle-completion. For Revoke that means after `state.shutdown()`
returns; for ForceRefresh after `refresh()` returns successfully.

**Per-stream first poll uses `0`**, subsequent polls use `>` (per
`design.md` §7.3 step 5). The cloud tracks
`session_pel_drained:{sid}:{rid}` so the daemon doesn't need to do
this bookkeeping client-side.

## 9. Refresh scheduling

A separate `refresh_loop` task (replaces nothing — this is new):

```rust
async fn refresh_loop(
    client: HttpClient,
    state: StateHandle,
) {
    loop {
        let exp = client.access_token_exp().await;
        let now = Utc::now();
        let safety = Duration::from_secs(300); // 5 min
        let sleep_for = exp.signed_duration_since(now) - safety.into();
        tokio::select! {
            _ = state.shutdown_notified().notified() => return,
            _ = tokio::time::sleep(sleep_for.to_std().unwrap_or_default()) => {}
        }
        if let Err(e) = client.refresh().await {
            tracing::error!("scheduled refresh failed: {e:#}");
            // Fatal refresh errors trigger shutdown via the
            // refresh() implementation; transient errors (network
            // blip, 5xx) get one retry inside refresh() before
            // reaching here.
        }
    }
}
```

`force_refresh` from a poll response calls `client.force_refresh_inline()`
directly from `HttpLoop`'s dispatch path; it does not go through
this scheduler. The scheduler reads `access_token_exp()` after each
refresh so it picks up the new TTL.

## 10. Updated `Supervisor::run` skeleton

Annotated diff against `runner/src/daemon/supervisor.rs:56`:

```text
pub async fn run(self) -> Result<()> {
    // (1) workspace conflict checks — UNCHANGED.

    // (2) Build N RunnerInstances — UNCHANGED.
    //     But: drop the `out_tx` mpsc parameter. RunnerInstance now
    //     receives a `HttpClient` clone instead, which `RunnerOut`
    //     wraps internally.

    // (3) IPC handle — UNCHANGED.

    // (4) NEW: build HttpClient, mailboxes, status_sources,
    //     connected Notify.
    let client = HttpClient::new(creds, cloud_url)?;
    let mailboxes: Arc<RwLock<HashMap<Uuid, mpsc::Sender<...>>>> = ...;
    let status_sources: Arc<RwLock<HashMap<Uuid, RunnerStatusSource>>> = ...;
    let attach_runners: Arc<RwLock<AttachRunnerMap>> = ...;

    for inst in &instances {
        mailboxes.write().await.insert(inst.runner_id, inst.mailbox_tx.clone());
        status_sources.write().await.insert(
            inst.runner_id,
            RunnerStatusSource::from_state(&inst.state),
        );
        attach_runners.write().await.insert(
            inst.runner_id,
            AttachRunner {
                runner_id: inst.runner_id,
                state: inst.state.clone(),
                project_slug: inst.config.workspace.project_slug.clone(),
            },
        );
    }

    // (5) DROPPED: hb_handles loop. Status now folded into poll body.

    // (6) Spawn HttpLoop instead of ConnectionLoop:
    let connected = Arc::new(tokio::sync::Notify::new());
    let http_loop = HttpLoop {
        client: client.clone(),
        state: state.clone(),
        mailboxes: mailboxes.clone(),
        status_sources: status_sources.clone(),
        connected: connected.clone(),
    };
    let cloud_handle = tokio::spawn(http_loop.run());

    // (7) Spawn attach_emitter (replaces hello_emitter):
    let attach_handle = tokio::spawn(attach_emitter(
        attach_runners.clone(),
        connected.clone(),
        state.clone(),
        client.clone(),
    ));

    // (8) Spawn refresh_loop (NEW):
    let refresh_handle = tokio::spawn(refresh_loop(client.clone(), state.clone()));

    // (9) DROPPED: standalone Demux task. Demux logic lives inside
    //     HttpLoop::dispatch_response.

    // (10) Spawn one RunnerLoop per instance — UNCHANGED.

    // (11) Wait on shutdown signal — UNCHANGED.

    // (12) Send Bye via client.close_session() instead of out_tx.
    let _ = client.close_session().await;

    // (13) Abort tasks — same shape, different list:
    cloud_handle.abort();
    attach_handle.abort();
    refresh_handle.abort();
    for h in loop_handles { h.abort(); }
    ipc_handle.abort();
    Ok(())
}
```

## 11. Error / failure model

Per `design.md` §10, mapped to daemon-side response:

| Wire response                        | `HttpClient` action                                                                                                                                                                                                                                                                                                                                                                                                 | Visible to `RunnerLoop`?              |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| 401 `access_token_expired`           | call `refresh()`, retry once. Transparent.                                                                                                                                                                                                                                                                                                                                                                          | No.                                   |
| 401 `membership_revoked`             | `state.shutdown()`. Daemon dies.                                                                                                                                                                                                                                                                                                                                                                                    | Mailbox closes; loops exit.           |
| 401 `refresh_token_replayed`         | `state.shutdown()`. Daemon dies.                                                                                                                                                                                                                                                                                                                                                                                    | Mailbox closes; loops exit.           |
| 409 `session_evicted` on poll        | `state.shutdown()`. Daemon dies.                                                                                                                                                                                                                                                                                                                                                                                    | Mailbox closes; loops exit.           |
| 409 `session_evicted` on lifecycle   | Same — fatal. Another daemon owns the connection.                                                                                                                                                                                                                                                                                                                                                                   | Mailbox closes; loops exit.           |
| 409 `concurrent_poll`                | Internal logic error. Log + retry once after 100 ms backoff. Persistent failure → shutdown.                                                                                                                                                                                                                                                                                                                         | No (initially; alert ops on pattern). |
| 429 `poll_rate_exceeded`             | Backoff to ≥5 s between polls. Recovers.                                                                                                                                                                                                                                                                                                                                                                            | No.                                   |
| Network error / 5xx                  | Exponential backoff ≤30 s. Re-poll. If session is stale on recovery, `open_session()` gets a fresh one and `connected.notify_one()`.                                                                                                                                                                                                                                                                                | Brief silence; loops continue.        |
| `force_refresh` ServerMsg            | `force_refresh_inline()`; do not surface to instances.                                                                                                                                                                                                                                                                                                                                                              | No.                                   |
| `Revoke` ServerMsg (connection-wide) | Inline: `state.shutdown()`.                                                                                                                                                                                                                                                                                                                                                                                         | Mailbox closes; loops exit.           |
| `RemoveRunner` ServerMsg             | Demux to runner's mailbox. `RunnerLoop` exits its inner loop on this frame (same as today). On exit, the supervisor's join-handler removes the runner from the `mailboxes`, `status_sources`, and `attach_runners` maps and calls `client.detach_runner(runner_id)` to notify cloud. The other runners on the connection keep working. The RemoveRunner stream id is acked by `RunnerLoop` immediately before exit. | Yes — same as today.                  |
| Daemon crash mid-handler             | Process killed before `RunnerLoop` could send the ack. Stream id stays in `consumer-{sid}`'s PEL. New session's `attach/` XCLAIMs onto `consumer-{new_sid}`; first poll `XREADGROUP ... 0` redelivers; `InboundDedupe` lets the handler skip if it had partially run.                                                                                                                                               | Yes (re-delivery).                    |

## 12. Module-level test plan

- **`http.rs::HttpClient` unit tests**: mock `reqwest` server;
  refresh state machine (`expired → refresh → retry`); refresh
  failure modes (`replayed`, `membership_revoked`); session
  open/close; attach/detach.
- **`HttpLoop` unit tests**: ack-on-handle accumulation across
  multiple `PollResponseEntry`s for the same runner (list grows in
  arrival order; only ids whose handlers completed are present);
  ack drain happens at next poll, not at receive; `force_refresh`
  triggers inline refresh; `Revoke` triggers shutdown;
  `concurrent_poll` retry behavior.
- **`InboundDedupe` unit tests**: insert-and-seen returns true for
  same `mid`; LRU evicts oldest when full; distinct `mid` values
  pass through.
- **End-to-end ack-on-handle**: dispatch a poll containing 3
  messages for the same runner; assert that the next poll's
  `ack[<runner_id>]` only includes ids whose handlers have
  completed (block one handler in the test and confirm its id is
  absent).
- **`RunnerOut::send` dispatch table**: one test per `ClientMsg`
  variant, asserting it lands at the right `HttpClient` method.
  Effectively a `match`-completeness test — keeps future enum
  variants from silently being dropped.
- **`attach_emitter`**: re-attaches all registered runners on
  every fresh-session signal; idempotent under repeated fires.
- **Integration (against fake cloud)**: end-to-end Assign → Accept
  → RunEvent×N → RunCompleted; `force_refresh` mid-run; session
  eviction (`409`) triggers clean shutdown; cloud restart drops a
  poll, daemon recovers via re-poll.

## 13. Phase 4 sub-phases

The work decomposes naturally:

- **4a** — scaffold `runner/src/cloud/http.rs`. `HttpClient` with
  refresh, session open/close, attach, single integration test
  against a fake cloud. No supervisor changes yet; daemon still
  uses WS in this commit.
- **4b** — replace `ConnectionLoop` with `HttpLoop` in
  `Supervisor::run`. Move Demux into `HttpLoop::dispatch_response`.
  Adapt `RunnerOut::send`. End-to-end integration tests pass.
- **4c** — fold heartbeat into poll body. Drop `hb_handles`. Wire
  `status_sources` map. Per-runner liveness assertions in tests.
- **4d** — `attach_emitter` replaces `hello_emitter`. Verify
  reconnect re-attaches every runner.
- **4e** — `force_refresh` handler. Refresh-loop task. Bump
  `WIRE_VERSION`/`PROTOCOL_VERSION` to 4.

Each sub-phase is independently mergeable. **4a can ship behind a
runtime flag** (`PI_DASH_TRANSPORT=http|ws`) to allow side-by-side
validation against the cloud's dual-stack rollout in Phase 2/3.
Default flips to `http` at the end of 4e.

## 14. Open daemon-side questions

- **Worktree / per-run resource cleanup on `409 session_evicted`**:
  the daemon shuts down without sending lifecycle frames for any
  in-flight run. The cloud-side `Connection.revoke()` cascade
  (which is _not_ called on simple session-eviction) doesn't run,
  so the in-flight `AgentRun` rows stay `RUNNING` until heartbeat
  staleness reaps them. Acceptable in v1; document the latency.
- **Backpressure when a per-instance mailbox is slow**: today's
  Demux uses bounded `mpsc(...)` (capacity not in this excerpt).
  If the channel fills, `tx.send().await` parks the demux task,
  which parks the long-poll dispatcher, which delays acks. The
  per-runner ack pattern means a slow instance only delays its own
  cursor, but it's worth measuring under load before tuning.
- **HTTP/2 vs HTTP/1.1**: `reqwest` negotiates automatically. HTTP/2
  is preferable for keep-alive multiplexing of the many concurrent
  POSTs during active runs. Pin to `reqwest = { features = ["http2"] }`
  and verify in integration tests.

## 15. Out of scope for the daemon-side phase 4

- TUI redesign beyond the new `polling`/`refreshing`/`session_evicted`
  status indicators. The IPC shape (`StatusSnapshot`) does not
  change; existing TUI views render the new states automatically.
- `runner/src/cli/`-level changes. The `connect`/`config`/`tui`
  subcommands operate on credentials and config, both of which
  retain their existing shape (with the new `[refresh]` block per
  `design.md` §5.5).
- Changes to the agent bridge (`runner/src/codex/`,
  `runner/src/claude_code/`). Bridges interact with `RunnerOut`
  via the existing call-site shape and are unaffected by transport
  changes.
