# Cloud ↔ Daemon Transport: HTTPS Long-Poll — Tasks

Purpose: track the implementation of the HTTPS long-poll control plane and refresh-token authentication that replaces the always-on WebSocket transport.

Companion docs in this directory:

- `design.md` — architecture, wire protocol, decisions, phased rollout
- `daemon_module.md` — daemon-side architectural mapping (Phase 4 sub-design)

How to use this file:

- Keep task status in-place with checkboxes.
- Add PR links or issue ids inline after the task text.
- Do not delete completed tasks; strike or annotate only if scope changes.
- If a task expands materially, split it into a separate subtask block.
- Section refs like `design.md §5.3` point at the normative spec; treat them as the contract.

## Milestones

- [ ] Phase 1: cloud refresh-token + access-token issuance
- [ ] Phase 2: cloud sessions, attach, long-poll, Redis Streams outbox
- [ ] Phase 3: cloud HTTP endpoints for runner-upstream events
- [ ] Phase 4a: daemon `HttpClient` scaffold (refresh, session, attach)
- [ ] Phase 4b: daemon `HttpLoop` replaces `ConnectionLoop`; Demux relocation
- [ ] Phase 4c: daemon heartbeat → poll-body status folding
- [ ] Phase 4d: daemon `attach_emitter` replaces `hello_emitter`
- [ ] Phase 4e: daemon `force_refresh` handler + refresh scheduler + protocol-version bump
- [ ] Phase 5: cloud retires WS as control plane

## 1. Phase 1 — Cloud: refresh-token + access-token issuance

### 1.1. Schema

- [ ] Migration: rename `Connection.secret_hash` → `Connection.refresh_token_hash`
      Notes: `design.md` §6; same migration renames `secret_fingerprint` → `refresh_token_fingerprint`.
- [ ] Add `Connection.refresh_token_generation: PositiveIntegerField(default=0)`
- [ ] Add `Connection.previous_refresh_token_hash: CharField(max_length=128, blank=True, default="")`
      Notes: `design.md` §6 + decision #6; single-slot history used for replay detection at refresh time.
- [ ] Add `Connection.access_token_signing_key_version: PositiveIntegerField(default=1)`
      Notes: reserved; not consulted in v1 (`design.md` §5.2).

### 1.2. Access-token format & key ring

- [ ] Add `RUNNER_ACCESS_TOKEN_KEYS` setting in `apps/api/apple_pi_dash/settings/common.py`
      Notes: ordered key ring with `kid`, `secret`, `status` ∈ {`active`, `verify_only`}.
- [ ] Implement HS256 sign / verify helper isolated behind `tokens.access_token` module
      Notes: `design.md` §5.2 verification order — sig-by-kid → exp → rtg lower bound → optional `min_rtg`.
- [ ] Implement `Connection.refresh_token_generation` lower-bound check (one-generation grace)
- [ ] Add `RunnerForceRefresh` model (`connection`, `min_rtg`, `reason`, `created_at`)
      Notes: `design.md` §6 / §7.8; queried during access-token verification.

### 1.3. Refresh endpoint

- [ ] `POST /api/v1/runner/connections/<connection_id>/refresh/`
      Notes: `design.md` §5.3; bearer is the **refresh** token, not access.
- [ ] Logic in single `select_for_update` transaction (`design.md` §5.3):
  - [ ] Lookup by `id`; 404 / 401 `invalid_refresh_token` if missing.
  - [ ] 401 `connection_revoked` if `revoked_at` set.
  - [ ] Hash-match decision: try `refresh_token_hash` first, then
        `previous_refresh_token_hash`. Previous-match → call
        `connection.revoke()` and return 401 `refresh_token_replayed`;
        no match in either → 401 `invalid_refresh_token`.
  - [ ] Live `is_workspace_member(connection.created_by, connection.workspace_id)` — fail → `connection.revoke()`, 401 `membership_revoked`.
  - [ ] Atomic rotate: copy current hash → previous hash, set new
        hash, increment `refresh_token_generation`, mint new access
        token.
  - [ ] Delete any `RunnerForceRefresh` row for this connection
        (its directive is now satisfied by the new `rtg`).
  - [ ] Return both tokens + new generation.

### 1.4. `AccessTokenAuthentication` DRF class

- [ ] New class in `apps/api/pi_dash/runner/authentication.py`
      Notes: replaces `ConnectionBearerAuthentication` for new endpoints; old class stays for the WS dual-stack window.
- [ ] Verify signature by `kid` from key ring.
- [ ] Verify `exp`; on failure → 401 `access_token_expired`.
- [ ] Verify `rtg` against `connection.refresh_token_generation - 1`.
- [ ] Apply `RunnerForceRefresh.min_rtg` if a row exists for this connection.
- [ ] Set `request.auth_connection`, `request.auth_runner` (when URL kwargs name a runner).

### 1.5. Enrollment endpoint upgrade

- [ ] Modify `ConnectionEnrollEndpoint` (`apps/api/pi_dash/runner/views/connections.py:210-298`) to mint **both** tokens.
      Notes: `design.md` §5.1; response gains `refresh_token`, `access_token`, `access_token_expires_at`, `refresh_token_generation`, `long_poll_interval_secs`.
- [ ] Bump `connection.refresh_token_generation` to 1 on first enrollment.

### 1.6. Tests

- [ ] Refresh-endpoint state machine: revoked, replayed (previous-hash match), invalid (neither hash matches), membership-lost, happy path.
- [ ] Access-token verification: expired, bad signature, mismatched `rtg`, `min_rtg` rejection from a `RunnerForceRefresh` row.
- [ ] Key rotation: token signed with `verify_only` key still verifies; token signed with removed key fails.
- [ ] Refresh-token rotation: presenting the previous-generation token after rotation triggers `Connection.revoke()` cascade with reason `refresh_token_replayed`.
- [ ] Force-refresh clearing: after a `RunnerForceRefresh` row is set and then a successful refresh runs, the row is deleted and the new access token is accepted.
- [ ] Security: kick a workspace member, refresh fails, in-flight `AgentRun` cancelled, pinned QUEUED runs lose pin, pods re-drained.

## 2. Phase 2 — Cloud: sessions, attach, long-poll, Streams outbox

### 2.1. Schema

- [ ] Add `RunnerSession` model
      Notes: `design.md` §6; columns include `id`, `connection`, `created_at`, `last_seen_at`, `revoked_at`, `revoked_reason`, `protocol_version`, `host_label`, `agent_versions`.
- [ ] Add unique constraint: one active session per connection (`revoked_at IS NULL`).
- [ ] Add `RunMessageDedupe` model
      Notes: `design.md` decision #19; `(run_id, message_id)` unique; cleanup job for >7-day rows.

### 2.2. Session lifecycle endpoints

- [ ] `POST /api/v1/runner/connections/<cid>/sessions/`
      Notes: `design.md` §7.1; evicts prior session via `revoked_at = now` and pub/sub eviction signal; **does not** XCLAIM here.
- [ ] `DELETE /api/v1/runner/connections/<cid>/sessions/<sid>/`
      Notes: clean shutdown; reaps session and its streams after `2 × access_token_ttl_secs`.
- [ ] Idle reaper: sessions with no poll activity for `2 × long_poll_interval_secs` → marked revoked with reason `idle_timeout`.

### 2.3. Per-runner attach endpoint

- [ ] `POST /api/v1/runner/connections/<cid>/sessions/<sid>/runners/<rid>/attach/`
      Notes: `design.md` §7.2; mirrors `consumers.py:336-363` (validate runner, validate `project_slug`, populate authorised set, `_apply_hello`, mark online, drain queued runs, resume in-flight).
- [ ] Per-runner stream materialization (atomic):
  - [ ] `XGROUP CREATE runner_stream:{sid}:{rid} daemon-{sid} $ MKSTREAM` (idempotent on `BUSYGROUP`).
  - [ ] PEL claim from prior session's `runner_stream:{old_sid}:{rid}` if non-empty within retention window.
  - [ ] Drain `runner_offline_stream:{rid}` into the new session's stream; preserve original ids in metadata.
  - [ ] Return `welcome`, `stream_id`, `starting_id`.
- [ ] `DELETE /api/v1/runner/connections/<cid>/sessions/<sid>/runners/<rid>/`
      Notes: marks runner offline within session; runner row unchanged.
- [ ] On detach: publish `session_attach_change:<sid>` so any in-flight poll for the same session returns immediately with `messages: []` and the daemon's next poll uses the updated attached set (`design.md` §7.2).
- [ ] Verify: between detach completing and the next poll arriving, `enqueue_for_runner` for that runner routes to the offline stream (decision #18) rather than the now-detached session's stream.

### 2.4. Long-poll endpoint

- [ ] `POST /api/v1/runner/connections/<cid>/sessions/<sid>/poll`
      Notes: `design.md` §7.3; POST not GET because body carries `ack` map and `status[]` vector.
- [ ] Validate session is active; reject stale `session_id` with `409 session_evicted`.
- [ ] Update `RunnerSession.last_seen_at`.
- [ ] For each `status[]` entry: validate runner is attached (`400 unknown_runner_in_status` on miss), update `Runner.last_heartbeat_at`, run `_reap_stale_busy_runs`.
- [ ] Reject empty `status[]` when session has attached runners (`400 missing_runner_status`).
- [ ] For each `ack[]` entry: `XACK` the runner's stream consumer group.
- [ ] `XREADGROUP ... BLOCK 25000 STREAMS ... >` across all attached runners' streams.
- [ ] Return drained entries with `stream_id`, `mid`, `runner_id`, `type`, `body`.

### 2.5. Outbox helpers

- [ ] `enqueue_for_runner(runner_id, msg)` in `apps/api/pi_dash/runner/services/pubsub.py`
      Notes: `design.md` §7.4; `XADD` to active-session stream if attached; offline policy if not.
- [ ] Offline policy: reject `assign|cancel|decide|resume_ack` with `RunnerOfflineError`; queue control msgs in `runner_offline_stream:{rid}` with `MAXLEN ~ 1000`, 24h TTL.
- [ ] `read_for_session(sid, attached_rids, timeout_ms)` → `XREADGROUP`.
- [ ] `ack_for_session(sid, ack_map)` → per-runner `XACK`.
- [ ] Migrate `send_to_runner` to **dual-write** during the transition (Channels group + Redis stream).

### 2.6. Session-eviction & detach signaling

- [ ] Redis pub/sub channel `session_eviction:<cid>` published on `POST /sessions/` with body `{old_sid, new_sid}`.
- [ ] Redis pub/sub channel `session_attach_change:<sid>` published on per-runner attach/detach with body `{runner_id, op}`.
- [ ] Poll task structures itself as `tokio::select!` (Python `asyncio.wait`) over: `XREADGROUP BLOCK 25000`, `session_eviction:<cid>` subscription, `session_attach_change:<sid>` subscription, server timeout. First wake wins:
  - eviction → `409 session_evicted` with `superseded_by=<new_sid>`.
  - attach-change → return `messages: []` so daemon's next poll uses the new attached set.
  - timeout → return `messages: []` normally.
- [ ] On worker startup, ensure each in-flight poll's pub/sub subscription is created before the `XREADGROUP` blocks (otherwise an eviction firing in that gap is lost; the next poll's session-id check still catches it as a fallback).

### 2.7. Throttling

- [ ] `RunnerConnectionRateThrottle` keyed by `connection_id`
      Notes: `design.md` §9.1; 600 burst / 300 sustained per minute on upstream POSTs.
- [ ] Poll endpoint: protocol-bounded; optional 1-req/5s sustained backstop returning `429 poll_rate_exceeded`.
- [ ] Tighter throttle on enrollment + refresh keyed by connection + remote IP.

### 2.8. Sweepers / cleanup tasks (`design.md` §7.10)

- [ ] `sweep_idle_sessions` every 30s: revoke active sessions whose `last_seen_at` is older than `2 × long_poll_interval_secs` with reason `idle_timeout`; publish `session_eviction:<cid>` for each.
- [ ] `sweep_stale_runners` every 30s: flip `Runner.status = OFFLINE` for online runners whose `last_heartbeat_at` is older than `runner_offline_threshold_secs` (does not revoke; re-attach revives).
- [ ] `sweep_old_streams` every 5 min: `XGROUP DESTROY` + `DEL` per-runner streams of revoked sessions older than `2 × access_token_ttl_secs`; delete offline streams idle >24h with `XLEN == 0`.
- [ ] `sweep_run_message_dedupe` daily: delete rows older than `run_message_dedupe_ttl_secs` (7d).
- [ ] Wire all four to Celery beat (or chosen periodic scheduler); document expected execution time per run.

### 2.9. Tunables

- [ ] Add to `apple_pi_dash/settings/common.py`: `LONG_POLL_INTERVAL_SECS=25`, `ACCESS_TOKEN_TTL_SECS=3600`, `OFFLINE_STREAM_TTL_SECS=86400`, `OFFLINE_STREAM_MAXLEN=1000`, `ACTIVE_STREAM_MAXLEN=10000`, `RUN_MESSAGE_DEDUPE_TTL_SECS=604800`, `RUNNER_OFFLINE_THRESHOLD_SECS=50`, `EVENT_BATCH_MAX_AGE_MS=250`, `EVENT_BATCH_MAX_BYTES=65536`.

### 2.11. Protocol-version rejection (decision #14, `design.md` §7.10)

- [ ] `POST /sessions/` reads `X-Runner-Protocol-Version` header; missing or `< 4` → `426 Upgrade Required` with body `{"error": "protocol_version_unsupported", "minimum": 4, "upgrade_url": "..."}`.
- [ ] WS upgrade endpoint: reject `X-Runner-Protocol < 4` with WS close code 1008 reason `protocol_version_unsupported`.
- [ ] Test: v3 daemon hitting `POST /sessions/` gets 426; v3 daemon hitting WS upgrade gets 1008 close.

### 2.10. Tests

- [ ] Open session → attach runner → poll receives queued message → ack via next poll → message gone.
- [ ] Concurrent session-open evicts prior session; displaced poll returns `409 session_evicted`; new session inherits PEL via `XCLAIM` at attach.
- [ ] Per-runner liveness: omit one of two attached runners from `status[]`; sibling continues; omitted runner flips OFFLINE after 50s; stale busy-run reaping fires.
- [ ] Concurrent poll on same `session_id` → `409 concurrent_poll`.
- [ ] Offline enqueue rejected for `assign`; accepted for `config_push`; offline stream caps at `MAXLEN`.
- [ ] Daemon-crash scenario: message delivered via `XREADGROUP` but never acked → on next poll, `XREADGROUP ... 0` re-fetches it.

## 3. Phase 3 — Cloud: HTTP endpoints for runner-upstream events

### 3.1. Endpoint implementation

For each of `Accept`, `RunStarted`, `RunEvent`, `ApprovalRequest`, `RunAwaitingReauth`, `RunCompleted`, `RunPaused`, `RunFailed`, `RunCancelled`, `RunResumed`:

- [ ] `POST /api/v1/runner/runs/<run_id>/<verb>/`
      Notes: `design.md` §7.5; verbs as listed.
- [ ] Body schema mirrors today's WS frame (re-use serializers).
- [ ] `Idempotency-Key` header → `(run_id, message_id)` dedupe via `RunMessageDedupe`.

### 3.2. Run-level authorization

- [ ] Shared transport-service helper: `authorize_run_for_connection(run, connection)`
      Notes: `design.md` §7.5; require `run.runner.connection_id == request.auth_connection.id`; reject 403 `run_not_owned_by_connection`.
- [ ] Apply to every `/runs/<run_id>/...` endpoint via decorator or middleware.

### 3.3. Handler refactor

- [ ] Extract handler bodies from `RunnerConsumer.on_run_started`, `on_run_event`, `on_approval_request`, etc. into transport-agnostic services
      Notes: shared between WS path (still alive in phases 1–4) and new HTTP endpoints.

### 3.4. WS-upgrade ticket plumbing (`design.md` §7.9)

- [ ] `POST /api/v1/runner/runs/<run_id>/stream/upgrade/` mints a 60s ticket bound to `(run_id, stream, runner_id)`. Body `{"stream": "log" | "events"}`; runner_id resolved from run server-side.
- [ ] Storage: Redis key `ws_upgrade_ticket:{ticket_uuid}` with `EX 60`, body `{run_id, stream, runner_id, expires_at}`.
- [ ] WS handshake on `wss://.../stream/<ticket>` consumes the ticket via `GETDEL`. Reuse → reject. Missing → reject.
- [ ] v1 ships the endpoint and ticket store but no live consumer (deferred to first real use case per §14).

### 3.5. Tests

- [ ] One test per endpoint asserting same DB state changes as today's WS path produces.
- [ ] Cross-connection authz: a connection cannot post events for another connection's run → 403.
- [ ] Idempotency: same `Idempotency-Key` twice → second call is a no-op.
- [ ] WS-upgrade ticket: mint → consume → reuse rejected; expired ticket rejected.

## 4. Phase 4 — Daemon: switch the connection loop to HTTPS

Sub-phases per `daemon_module.md` §13. Each is independently mergeable.

### 4a. `HttpClient` scaffold

- [ ] New module `runner/src/cloud/http.rs` with `HttpClient`, `HttpClientInner`, `AccessToken`, `SessionState`.
- [ ] `reqwest` client with HTTP/2 keep-alive enabled.
- [ ] Methods: `refresh()`, `open_session()`, `attach_runner()`, `detach_runner()`, `close_session()`.
- [ ] Persist refresh token to disk **before** discarding old one in memory.
- [ ] Credentials file gains `[refresh]` block (`token`, `generation`, `issued_at`); migration code on first run.
- [ ] 401 `access_token_expired` → auto-refresh + retry once.
- [ ] 401 `membership_revoked` / `refresh_token_replayed` → propagate fatal; daemon shuts down.
- [ ] Integration test against fake cloud: enroll-like flow → refresh → open_session → attach → close_session.

### 4b. `HttpLoop` replaces `ConnectionLoop`; Demux relocation

- [ ] New `HttpLoop` struct in `runner/src/cloud/http.rs` carrying `client`, `state`, `mailboxes`, `status_sources`, `connected`.
- [ ] `HttpLoop::poll_once` builds `ack` and `status[]` from internal state and calls `HttpClient::poll`.
- [ ] `HttpLoop::dispatch_response` ports the demux logic from `supervisor.rs:268-312`:
  - [ ] Routes `Some(rid)` → mailbox; logs+drops on unknown rid.
  - [ ] Routes `None` → connection-scoped handlers (Revoke, deprecated Ping, future force_refresh).
- [ ] Adapt `RunnerOut::send` to call `HttpClient::dispatch_client_msg` (variant → URL routing per `daemon_module.md` §3 table).
- [ ] `RunnerOut::send_connection_scoped`: `Bye` → `client.close_session()`; assert any other variant is unreachable.
- [ ] Replace `cloud_handle` in `Supervisor::run` with `HttpLoop` task; drop the `out_tx`/`out_rx` mpsc.
- [ ] Drop the standalone `demux` task; logic now inside `HttpLoop`.
- [ ] Recovery on transient errors: exponential backoff ≤30s; on session-stale (network blip, **not** `409`), `open_session()` + `connected.notify_one()`. `409 session_evicted` is fatal and triggers shutdown.
- [ ] On `RunnerLoop` exit (e.g. due to `RemoveRunner`), the supervisor's join-handler removes the runner from `mailboxes`/`status_sources`/`attach_runners` maps and calls `client.detach_runner(runner_id)` (`daemon_module.md` §11).
- [ ] Ack-on-receive: track `ack_map` inside `HttpLoop`, submit on next poll.
- [ ] End-to-end integration test: assign → accept → run-event → completed over HTTP.

### 4c. Heartbeat → poll-body status folding

- [ ] New `RunnerStatusSource` thin wrapper around `state.rx_status` + `state.rx_in_flight` watch receivers.
- [ ] `Supervisor::run` populates `status_sources: Arc<RwLock<HashMap<Uuid, RunnerStatusSource>>>` map at instance setup.
- [ ] `HttpLoop::poll_once` snapshots the map → `status[]` vector with current timestamp.
- [ ] Drop `hb_handles` Vec, the per-instance `tokio::time::interval` heartbeat tasks, and the `Heartbeat` ClientMsg dispatch path.
- [ ] Property: cloud-side `_reap_stale_busy_runs` continues to fire correctly under the new status flow.
- [ ] Integration test: simulate one runner going silent (omitted from status); confirm cloud flips it OFFLINE; sibling unaffected.

### 4d. `attach_emitter` replaces `hello_emitter`

- [ ] New `attach_emitter` function shaped like today's `hello_emitter` (`supervisor.rs:401`); fires on `connected.notified()`.
- [ ] Loops every registered `AttachRunner` and calls `client.attach_runner(...)`.
- [ ] `HttpLoop` fires `connected.notify_one()` after every successful `open_session()`.
- [ ] Drop `hello_emitter`, `HelloRunnerMap`, and the `Hello` ClientMsg dispatch path.
- [ ] Integration test: simulate fresh session-open after network blip; every runner is re-attached idempotently.

### 4e. `force_refresh` + refresh scheduler + protocol-version bump

- [ ] Add `ServerMsg::ForceRefresh { reason, min_rtg }` variant to `runner/src/cloud/protocol.rs`.
- [ ] `HttpLoop::dispatch_response` handles `ForceRefresh` inline by calling `client.force_refresh_inline()`; does **not** propagate to instances.
- [ ] New `refresh_loop` task spawned by `Supervisor::run`; sleeps until `access_token.exp - 5min`, then calls `client.refresh()`.
- [ ] Bump `WIRE_VERSION` / `PROTOCOL_VERSION` to **4**; cloud-side `426 upgrade required` for v3 daemons.
- [ ] TUI / IPC `StatusSnapshot` surfaces `polling` / `refreshing` / `session_evicted` states.
- [ ] Runtime flag `PI_DASH_TRANSPORT=http|ws` (defaults to `http` after this sub-phase merges; `ws` allowed only during validation window).
- [ ] Integration test: queue a `force_refresh` server-side; daemon refreshes inline before next normal-cycle refresh.

## 5. Phase 5 — Cloud: retire WS as control plane

### 5.1. Code removal

- [ ] `send_to_runner` stops dual-writing — Redis stream only.
- [ ] Remove `receive_json` control-message hot path from the Channels consumer.
- [ ] Keep the consumer mounted **only** for the per-run upgrade ticket path (`design.md` §7.9); gate the handshake on the upgrade ticket.
- [ ] Remove `ConnectionBearerAuthentication` from new endpoints (it was kept around for dual-stack).

### 5.2. Shared service extraction

- [ ] Extract `_apply_hello`, `_resolve_connection_runner`, group-add, online/offline transitions from `consumers.py` into a shared service module callable from both the WS consumer and the HTTP `attach/` endpoint.

### 5.3. Verification

- [ ] Monitoring dashboard: WS endpoint control traffic over a 24h window.
- [ ] Phase-5 done bar: zero control traffic on the WS endpoint; only upgrade-ticket handshakes.

## 6. Cross-cutting

### 6.1. Observability

- [ ] Metrics: poll latency (p50/p99), poll empty-vs-non-empty ratio, refresh count/rate, `force_refresh` queue depth, session evictions/min, `XCLAIM` count, offline-stream entries dropped.
- [ ] Logs: every session lifecycle event (`open`, `evict`, `delete`, `idle_timeout`); every refresh outcome with reason on failure.
- [ ] Alert: empty `status[]` from a session with attached runners — see `design.md` §7.7 open question for threshold.

### 6.2. Documentation

- [ ] Update `runner/README.md` configure → service install → tui flow to reference the new credentials format.
- [ ] Note the `PI_DASH_TRANSPORT` runtime flag in operator docs (briefly; it's transitional).

### 6.3. Security

- [ ] Confirm refresh-token-on-disk path remains 0600 after the credentials-file format migration.
- [ ] Document the access-token-staleness window (≤ TTL) in operator-facing security notes.

### 6.4. Open questions to resolve before / during implementation

- [ ] Outbox stream keying revisit if `MAX_RUNNERS_PER_MACHINE` ceiling is ever raised (`design.md` §13).
- [ ] Switch to Ed25519 access-token signing if a sidecar verifier appears (`design.md` §13).
- [ ] Refresh-rotation replay-window width — start at 1 generation; widen to 2 if spurious leak detections emerge in production.
- [ ] Per-run WS upgrade ticket lifetime — currently 60s; tighten or loosen after first real consumer.
- [ ] Mailbox backpressure under load — measure during phase-4 integration testing; tune mpsc capacity if needed (`daemon_module.md` §14).

## Deferred / out of scope for v1

- Live log streaming via the per-run WS upgrade path (the canonical use case for `design.md` §7.9). Designed for, not built in v1.
- Multi-region cloud (single-Redis outbox; cross-region replication left for later).
- SSE / WebTransport push transports.
