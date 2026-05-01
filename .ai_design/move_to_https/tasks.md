# Runner ↔ Cloud Transport: HTTPS Long-Poll — Tasks

Purpose: track the implementation of the per-runner HTTPS long-poll
control plane, refresh-token authentication, and `MachineToken` CLI
credential that replace the always-on WebSocket transport and the
`Connection`-as-trust-unit data model.

Companion docs in this directory:

- `design.md` — architecture, wire protocol, decisions, phased rollout
- `daemon_module.md` — daemon-side architectural mapping (Phase 4
  sub-design)

How to use this file:

- Keep task status in-place with checkboxes.
- Add PR links or issue ids inline after the task text.
- Do not delete completed tasks; strike or annotate only if scope
  changes.
- Section refs like `design.md §5.3` point at the normative spec;
  treat them as the contract.

## Milestones

- [ ] Phase 1: cloud — drop Connection, Runner gains trust fields,
      per-runner refresh + access-token issuance, `MachineToken`
- [ ] Phase 2: cloud — per-runner sessions, per-runner streams,
      long-poll
- [ ] Phase 3: cloud — HTTP endpoints for runner-upstream events
- [ ] Phase 4a: daemon — `SharedHttpTransport` + `RunnerCloudClient`
      scaffold
- [ ] Phase 4b: daemon — per-runner `HttpLoop` replaces shared
      `ConnectionLoop`; Demux removal
- [ ] Phase 4c: daemon — heartbeat → poll-body `status` folding;
      multi-runner isolation tests
- [ ] Phase 4d: daemon — `force_refresh` handler + per-runner
      refresh scheduler + protocol-version bump
- [ ] Phase 5: cloud — retire WS as control plane

## 1. Phase 1 — Cloud: Runner-as-trust-unit + tokens + MachineToken

### 1.1 Schema migration (single migration, no production data)

- [ ] Drop `Connection` table.
- [ ] Add to `Runner` (`apps/api/pi_dash/runner/models.py`):
  - [ ] `created_by` FK → User
  - [ ] `workspace` FK → Workspace (denormalized from
        `runner.pod.project.workspace` for fast revocation queries)
  - [ ] `refresh_token_hash` indexed CharField(128)
  - [ ] `refresh_token_fingerprint` CharField(8)
  - [ ] `refresh_token_generation` PositiveIntegerField, default 0
  - [ ] `previous_refresh_token_hash` CharField(128, blank=True,
        default="") — single-slot history, `design.md` §5.3
  - [ ] `access_token_signing_key_version` PositiveIntegerField,
        default 1 (reserved; not consulted in v1)
  - [ ] `revoked_at` DateTimeField (null)
  - [ ] `revoked_reason` CharField(32, blank). Values:
        `manual_revoke`, `membership_revoked`,
        `refresh_token_replayed`, `runner_removed`.
  - [ ] `enrolled_at` DateTimeField, `auto_now_add`
- [ ] Re-key `RunnerSession` from `(connection)` to `(runner)`:
  - [ ] FK `runner` (replaces `connection`)
  - [ ] Unique constraint:
        `UniqueConstraint(fields=["runner"], condition=Q(revoked_at__isnull=True))`
- [ ] Re-key `RunnerForceRefresh` from `(connection)` to `(runner)`:
  - [ ] OneToOneField `runner` (replaces `connection`)
- [ ] Add `MachineToken` model (see §1.5).

### 1.2 Access-token format & key ring

- [ ] Add `RUNNER_ACCESS_TOKEN_KEYS` setting in
      `apps/api/apple_pi_dash/settings/common.py` — ordered key ring
      with `kid`, `secret`, `status` ∈ {`active`, `verify_only`}.
- [ ] HS256 sign / verify helper in `tokens.access_token` module.
- [ ] Verification order (`design.md` §5.2): sig-by-kid → exp →
      rtg lower bound (against `Runner.refresh_token_generation - 1`)
      → optional `RunnerForceRefresh.min_rtg`.
- [ ] Access-token payload includes `sub=runner_id`, `uid=user_id`,
      `wid=workspace_id`, `iat`, `exp`, `rtg`, `kid`.

### 1.3 Refresh endpoint

- [ ] `POST /api/v1/runner/runners/<runner_id>/refresh/`
      Notes: `design.md` §5.3; bearer is the **refresh** token, not
      access.
- [ ] Logic in single `select_for_update` transaction on `Runner`
      row:
  - [ ] Lookup by `id`; 404 / 401 `invalid_refresh_token` if missing.
  - [ ] 401 `runner_revoked` if `revoked_at` set.
  - [ ] Hash-match decision: try `refresh_token_hash` first, then
        `previous_refresh_token_hash`. Previous-match → call
        `runner.revoke()` and return 401 `refresh_token_replayed`;
        no match in either → 401 `invalid_refresh_token`.
  - [ ] Live `is_workspace_member(runner.created_by, runner.workspace_id)`
        — fail → `runner.revoke()`, 401 `membership_revoked`.
  - [ ] Atomic rotate: copy current hash → previous, set new hash,
        increment `refresh_token_generation`, mint new access token.
  - [ ] Delete any `RunnerForceRefresh` row for this runner.
  - [ ] Return both tokens + new generation.

### 1.4 Authentication classes

- [ ] `RunnerAccessTokenAuthentication` in
      `apps/api/pi_dash/runner/authentication.py`:
  - [ ] Bearer JWT, verify by `kid`.
  - [ ] Verify `exp`; on failure → 401 `access_token_expired`.
  - [ ] Verify `rtg` against `Runner.refresh_token_generation - 1`.
  - [ ] Apply `RunnerForceRefresh.min_rtg` if a row exists for this
        runner.
  - [ ] For runner-scoped URLs (`<rid>` in path): assert
        `token.sub == url_runner_id`; 403 `runner_id_mismatch` else.
  - [ ] Set `request.auth_runner`.
- [ ] `MachineTokenAuthentication` (§1.5) — separate class for
      `/api/v1/` user-action endpoints.
- [ ] Old `ConnectionBearerAuthentication` is **removed entirely**
      (decision #13 — no migration); the WS path stays mounted but
      no longer participates in control-plane auth (Phase 5 keeps
      it for upgrade-ticket handshakes).

### 1.5 MachineToken model + auth

- [ ] Add `MachineToken` model in
      `apps/api/pi_dash/runner/models.py` (or new module):
  - [ ] `id` UUID PK
  - [ ] `user` FK → User
  - [ ] `workspace` FK → Workspace
  - [ ] `host_label` CharField(255)
  - [ ] `token_hash` indexed hash
  - [ ] `token_fingerprint` char(8)
  - [ ] `label` CharField (auto-generated `"machine: <host_label>"`)
  - [ ] `created_at`, `revoked_at` (null), `last_used_at` (null)
  - [ ] `is_service` BooleanField default True
  - [ ] Unique constraint:
        `UniqueConstraint(fields=["user", "workspace", "host_label"], condition=Q(revoked_at__isnull=True))`
- [ ] `MachineTokenAuthentication` DRF class:
  - [ ] Bearer hash-match.
  - [ ] 401 `machine_token_invalid` / `machine_token_revoked`.
  - [ ] Per-request `is_workspace_member` check (no refresh
        chokepoint for PATs); membership lapse → set `revoked_at`,
        return 401 `membership_revoked`.
  - [ ] Best-effort `last_used_at` update.
  - [ ] Throttle: `ServiceTokenRateThrottle` (300/min).
- [ ] Web-UI list/revoke page for the user's MachineTokens per
      workspace (out-of-scope HTML; just the API endpoint to list +
      revoke is in scope here).
- [ ] `pidash auth login` flow:
  - [ ] Web UI mints a one-time ticket bound to `(user, workspace)`.
  - [ ] CLI `pidash auth login <ticket>` calls
        `POST /api/v1/runner/machine-tokens/` to redeem the ticket
        for a MachineToken; daemon writes
        `~/.config/apple-pi-dash-runner/machine_token.toml` (0600).

### 1.6 Enrollment endpoint

- [ ] Move `POST /api/v1/runner/connections/enroll/` →
      `POST /api/v1/runner/runners/enroll/`.
- [ ] Mint refresh + access tokens for the new `Runner` row;
      response shape per `design.md` §5.1.
- [ ] **MachineToken bootstrap**: check for a live `MachineToken`
      matching `(user, workspace, host_label)`. If none, mint one
      and include it in the response under `machine_token`.
      Otherwise omit the field.
- [ ] Bump new runner's `refresh_token_generation` to 1 on first
      enrollment.

### 1.7 Tests

- [ ] Refresh-endpoint state machine (per-runner): revoked,
      replayed (previous-hash match), invalid (neither hash matches),
      membership-lost, happy path.
- [ ] Access-token verification: expired, bad signature, mismatched
      `rtg`, `min_rtg` rejection from a `RunnerForceRefresh` row,
      `runner_id_mismatch` on URL/sub mismatch.
- [ ] Key rotation: token signed with `verify_only` key still
      verifies; token signed with removed key fails.
- [ ] Refresh-token rotation: presenting the previous-generation
      token after rotation triggers `Runner.revoke()` cascade with
      reason `refresh_token_replayed`.
- [ ] Force-refresh clearing: after a `RunnerForceRefresh` row is
      set and a successful refresh runs, the row is deleted and the
      new access token is accepted.
- [ ] Security: kick a workspace member, every Runner owned by them
      in that workspace fails its next refresh and self-revokes;
      in-flight `AgentRun` cancelled, pinned QUEUED runs lose pin,
      pods re-drained.
- [ ] MachineToken bootstrap: first enrollment per
      `(user, workspace, host_label)` returns a token; second
      enrollment from same machine omits the field.
- [ ] MachineToken auth: revoked token → 401; live token + lapsed
      membership → 401 + `revoked_at` set on the next request.
- [ ] `pidash auth login` flow: mint ticket → redeem → MachineToken
      issued; reuse rejected; expired ticket rejected.

## 2. Phase 2 — Cloud: per-runner sessions + streams + long-poll

### 2.1 Session lifecycle endpoints

- [ ] `POST /api/v1/runner/runners/<rid>/sessions/`
      Notes: `design.md` §7.1; combines today's session-open and
      per-runner Hello into one step.
  - [ ] Verify access token; assert `token.sub == rid`.
  - [ ] Evict any prior active session for this runner; publish
        `session_eviction:<rid>` pub/sub.
  - [ ] Ensure `runner_stream:{rid}` + `runner-group:{rid}`:
        `XGROUP CREATE runner_stream:{rid} runner-group:{rid} $ MKSTREAM`
        (ignore `BUSYGROUP`).
  - [ ] Reassign prior consumer's PEL onto `consumer-{new_sid}` via
        paginated `XAUTOCLAIM` loop (loop until
        `next_cursor == "0-0"`; `XPENDING ... COUNT N` is unsafe
        because PEL may exceed any single page).
  - [ ] Validate `project_slug` matches
        `runner.pod.project.identifier` if provided.
  - [ ] Apply `_apply_hello` (metadata + stale-busy reaping).
  - [ ] Mark runner ONLINE.
  - [ ] Drain queued runs (`drain_for_runner_by_id`).
  - [ ] Drain `runner_offline_stream:{rid}` into the live stream
        oldest-first.
  - [ ] If `in_flight_run` was set, kick off `_resume_run` and
        include `resume_ack` in the response.
  - [ ] Create the `RunnerSession` row.
  - [ ] Return `welcome` payload (and optional `resume_ack`).
- [ ] `DELETE /api/v1/runner/runners/<rid>/sessions/<sid>/`
      Notes: clean shutdown; reaps session row + consumer ownership
      after `2 × access_token_ttl_secs`; persistent stream/group
      survive.
- [ ] Idle reaper: sessions with no poll activity for
      `2 × long_poll_interval_secs` → marked revoked with reason
      `idle_timeout`.

### 2.2 Long-poll endpoint

- [ ] `POST /api/v1/runner/runners/<rid>/sessions/<sid>/poll`
      Notes: `design.md` §7.3; POST not GET because body carries
      `ack` list and `status` entry.
- [ ] Validate session is active for this runner; reject stale
      `session_id` with `409 session_evicted`.
- [ ] Update `RunnerSession.last_seen_at`.
- [ ] Apply `status` (single entry — this runner's): update
      `Runner.last_heartbeat_at`, run
      `_reap_stale_busy_runs(runner, status)`.
- [ ] If `ack` is non-empty:
      `XACK runner_stream:{rid} runner-group:{rid} <id1> [<id2> ...]`
      (XACK takes exact id list, not range — `design.md` decision #9).
- [ ] Per-session id-marker selection: track
      `session_pel_drained:{sid}` (Redis SET / boolean) — first poll
      after session-open reads with `0`, subsequent polls with `>`.
- [ ] Issue one `XREADGROUP GROUP runner-group:{rid} consumer-{sid}
    COUNT 100 BLOCK 25000 STREAMS runner_stream:{rid} <0|>>`.
- [ ] Return drained entries with `stream_id`, `mid`, `type`, `body`.

### 2.3 Outbox helpers

- [ ] `enqueue_for_runner(runner_id, msg)` in
      `apps/api/pi_dash/runner/services/pubsub.py`:
  - [ ] If runner has active session: `XADD runner_stream:{rid}
    {...msg}` (no inline `MAXLEN` — see retention contract in
        `design.md` §7.4).
  - [ ] Else: offline policy. Reject `assign|cancel|decide|resume_ack`
        with `RunnerOfflineError`; queue control msgs in
        `runner_offline_stream:{rid}` with `MAXLEN ~ 1000`, 24h TTL.
        (Offline streams have no consumer group / PEL, so `MAXLEN`
        is safe here.)
- [ ] `read_for_session(rid, sid, timeout_ms)` →
      `XREADGROUP GROUP runner-group:{rid} consumer-{sid} ... BLOCK
    timeout_ms STREAMS runner_stream:{rid} (0|>)`.
- [ ] `ack_for_session(rid, [stream_id, ...])` →
      `XACK runner_stream:{rid} runner-group:{rid} <id1> [<id2> ...]`
      (multi-id form).
- [ ] Migrate `send_to_runner` to **dual-write** during the
      transition (Channels group + per-runner Redis stream).

### 2.4 Session-eviction signaling

- [ ] Redis pub/sub channel `session_eviction:<rid>` published on
      `POST /sessions/` with body `{old_sid, new_sid}`.
- [ ] Poll task structures itself as `tokio::select!` (Python
      `asyncio.wait`) over: `XREADGROUP BLOCK 25000`,
      `session_eviction:<rid>` subscription, server timeout. First
      wake wins:
  - eviction → `409 session_evicted` with `superseded_by=<new_sid>`.
  - timeout → return `messages: []` normally.
- [ ] On worker startup, ensure the pub/sub subscription is created
      before `XREADGROUP` blocks (otherwise an eviction firing in
      that gap is lost; the next poll's session-id check still
      catches it as a fallback).

### 2.5 Throttling

- [ ] `RunnerRateThrottle` keyed by `runner_id`
      Notes: `design.md` §9.1; 600 burst / 300 sustained per minute
      on upstream POSTs.
- [ ] Poll endpoint: protocol-bounded; optional 1-req/5s sustained
      backstop returning `429 poll_rate_exceeded`.
- [ ] Tighter throttle on enrollment + refresh keyed by remote IP.

### 2.6 Sweepers / cleanup tasks (`design.md` §7.10)

- [ ] `sweep_idle_sessions` every 30s: revoke active sessions whose
      `last_seen_at` is older than `2 × long_poll_interval_secs`
      with reason `idle_timeout`; publish `session_eviction:<rid>`
      for each.
- [ ] `sweep_stale_runners` every 30s: flip `Runner.status = OFFLINE`
      for online runners whose `last_heartbeat_at` is older than
      `runner_offline_threshold_secs` (does not revoke; re-attach
      revives).
- [ ] `sweep_old_streams` every 5 min: three jobs per `design.md`
      §7.10.
  1. **Old-consumer reaping**: for each revoked session older than
     `2 × access_token_ttl_secs`, paginated `XAUTOCLAIM` of any
     still-pending entries to the successor consumer if one exists,
     else `XGROUP DELCONSUMER runner_stream:{rid}
runner-group:{rid} consumer-{sid}`. The persistent
     `runner_stream:{rid}` and `runner-group:{rid}` are NOT
     destroyed.
  2. **PEL-aware trim**: `XPENDING runner_stream:{rid}
runner-group:{rid}` (summary form) → `min_pending_id`; compute
     `safe_cutoff = min(time_cutoff_id, min_pending_id - 1)`;
     `XTRIM runner_stream:{rid} MINID <safe_cutoff>` (exact MINID,
     not approximate). Skip trim if the resulting cutoff is
     non-monotonic.
  3. **Orphaned-stream deletion**: delete `runner_stream:{rid}`
     whose runner is revoked or has been idle with `XLEN == 0` for
     > 24h. Delete `runner_offline_stream:{rid}` idle >24h with
     > `XLEN == 0`.
- [ ] `sweep_run_message_dedupe` daily: delete rows older than
      `run_message_dedupe_ttl_secs` (7d).
- [ ] Wire all four to Celery beat (or chosen periodic scheduler);
      document expected execution time per run.

### 2.7 Tunables

- [ ] Add to `apple_pi_dash/settings/common.py`:
      `LONG_POLL_INTERVAL_SECS=25`, `ACCESS_TOKEN_TTL_SECS=3600`,
      `OFFLINE_STREAM_TTL_SECS=86400`,
      `OFFLINE_STREAM_MAXLEN=1000`,
      `RUNNER_STREAM_MIN_RETENTION_SECS=3600`,
      `RUN_MESSAGE_DEDUPE_TTL_SECS=604800`,
      `RUNNER_OFFLINE_THRESHOLD_SECS=50`,
      `EVENT_BATCH_MAX_AGE_MS=250`, `EVENT_BATCH_MAX_BYTES=65536`.

### 2.8 Protocol-version rejection (decision #14)

- [ ] `POST /sessions/` reads `X-Runner-Protocol-Version` header;
      missing or `< 4` → `426 Upgrade Required` with body
      `{"error": "protocol_version_unsupported", "minimum": 4,
    "upgrade_url": "..."}`.
- [ ] WS upgrade endpoint: reject `X-Runner-Protocol < 4` with WS
      close code 1008 reason `protocol_version_unsupported`.
- [ ] Test: v3 daemon hitting `POST /sessions/` gets 426; v3 daemon
      hitting WS upgrade gets 1008 close.

### 2.9 Tests

- [ ] Open session for runner R → poll receives queued message →
      ack via next poll → message gone.
- [ ] Concurrent session-open for R evicts prior session; displaced
      poll returns `409 session_evicted`. New session-open
      reassigns the prior consumer name's PEL onto the new consumer
      name via paginated `XAUTOCLAIM` (within the same persistent
      runner stream + group); first poll under new session
      re-fetches via `XREADGROUP ... 0`. Variant: prior consumer's
      PEL > 1000 entries — verify all are reassigned.
- [ ] **Multi-runner isolation**: open sessions for R₁ and R₂ on the
      same daemon; evict R₁'s session — confirm R₂'s session
      unaffected.
- [ ] Concurrent poll on same `session_id` → `409 concurrent_poll`.
- [ ] Offline enqueue rejected for `assign`; accepted for
      `config_push`; offline stream caps at `MAXLEN`.
- [ ] Daemon-crash scenario: message delivered via `XREADGROUP` but
      never acked → on next poll, `XREADGROUP ... 0` re-fetches it.
- [ ] Per-runner liveness: stop polling for one runner; sibling
      continues; stalled runner flips OFFLINE after 50s; stale
      busy-run reaping fires.

## 3. Phase 3 — Cloud: HTTP endpoints for runner-upstream events

### 3.1 Endpoint implementation

For each of `Accept`, `RunStarted`, `RunEvent`, `ApprovalRequest`,
`RunAwaitingReauth`, `RunCompleted`, `RunPaused`, `RunFailed`,
`RunCancelled`, `RunResumed`:

- [ ] `POST /api/v1/runner/runs/<run_id>/<verb>/`
      Notes: `design.md` §7.5.
- [ ] Body schema mirrors today's WS frame (re-use serializers).
- [ ] `Idempotency-Key` header → `(run, message_id)` dedupe via
      `RunMessageDedupe`.

### 3.2 Run-level authorization

- [ ] Shared transport-service helper:
      `authorize_run_for_runner(run, runner)` —
      `design.md` §7.5: require
      `run.runner_id == request.auth_runner.id`; reject 403
      `run_not_owned_by_runner`.
- [ ] Apply to every `/runs/<run_id>/...` endpoint via decorator or
      middleware.

### 3.3 Handler refactor

- [ ] Extract handler bodies from `RunnerConsumer.on_run_started`,
      `on_run_event`, `on_approval_request`, etc. into
      transport-agnostic services callable from both WS path
      (during phases 1–4) and new HTTP endpoints.

### 3.4 WS-upgrade ticket plumbing (`design.md` §7.9)

- [ ] `POST /api/v1/runner/runs/<run_id>/stream/upgrade/` mints a
      60s ticket bound to `(run_id, stream, runner_id)`. Body
      `{"stream": "log" | "events"}`; `runner_id` resolved from
      `run` server-side.
- [ ] Storage: Redis key `ws_upgrade_ticket:{ticket_uuid}` with
      `EX 60`, body `{run_id, stream, runner_id, expires_at}`.
- [ ] WS handshake on `wss://.../stream/<ticket>` consumes the
      ticket via `GETDEL`. Reuse → reject. Missing → reject.
- [ ] v1 ships the endpoint and ticket store but no live consumer
      (deferred to first real use case per `design.md` §14).

### 3.5 Tests

- [ ] One test per endpoint asserting same DB state changes as
      today's WS path produces.
- [ ] Cross-runner authz: a runner cannot post events for another
      runner's run → 403.
- [ ] Idempotency: same `Idempotency-Key` twice → second call is a
      no-op.
- [ ] WS-upgrade ticket: mint → consume → reuse rejected; expired
      ticket rejected.

## 4. Phase 4 — Daemon: per-runner HTTPS clients

Sub-phases per `daemon_module.md` §13. Each is independently
mergeable behind `PI_DASH_TRANSPORT=http|ws` until 4d defaults to
`http`.

### 4a. `SharedHttpTransport` + `RunnerCloudClient` scaffold

- [ ] New module `runner/src/cloud/http.rs` with
      `SharedHttpTransport`, `RunnerCloudClient`,
      `RunnerCloudClientInner`, `AccessToken`, `SessionState`.
- [ ] `SharedHttpTransport`: clone-shared `reqwest::Client` with
      HTTP/2 keep-alive enabled.
- [ ] `RunnerCloudClient` methods: `refresh()`, `open_session()`,
      `close_session()` (poll, post_run_event, post_run_lifecycle,
      post_approval_request come in 4b).
- [ ] **Single-flight refresh** per `RunnerCloudClient` via
      `tokio::sync::watch`-based gate (`daemon_module.md` §9).
- [ ] Per-runner credentials handle reads/writes
      `~/.config/apple-pi-dash-runner/runners/<rid>/credentials.toml`
      (0600). Persist new refresh token to disk **before** discarding
      old one in memory. Atomic write-temp + rename.
- [ ] Daemon-level `daemon.toml` for shared config (host_label,
      cloud URL).
- [ ] Daemon-level `machine_token.toml` written by `pidash auth
    login` flow (read-only as far as the daemon is concerned —
      MachineToken is for the CLI, not the daemon).
- [ ] 401 `access_token_expired` → auto-refresh + retry once.
- [ ] 401 `membership_revoked` / `refresh_token_replayed` /
      `runner_revoked` / `runner_id_mismatch` → propagate fatal;
      this `RunnerInstance` shuts down (siblings continue).
- [ ] Integration test against fake cloud: enroll-like flow →
      refresh → open_session → close_session for a single runner.

### 4b. Per-runner `HttpLoop` replaces `ConnectionLoop`

- [ ] New `HttpLoop` struct in `runner/src/cloud/http.rs` carrying
      `client`, `state` (daemon-level), `runner_state`, `mailbox`,
      `ack_rx`.
- [ ] `HttpLoop::run`: ensure access token → `open_session()` →
      hand `Welcome` (and optional `resume_ack`) to mailbox →
      poll forever.
- [ ] `HttpLoop::poll_once` builds `ack` and `status` (single entry)
      from the runner's state, calls `RunnerCloudClient::poll`.
- [ ] `HttpLoop::dispatch_response`:
  - [ ] `ServerMsg::ForceRefresh` → `client.force_refresh_inline()`,
        ack inline.
  - [ ] `ServerMsg::Revoke` → `runner_state.shutdown()`, ack inline,
        return.
  - [ ] Everything else → `mailbox.send(env)` (RunnerLoop acks
        post-handle).
- [ ] Adapt `RunnerOut::send` to call
      `RunnerCloudClient::dispatch_client_msg` (variant → URL
      routing per `daemon_module.md` §3 table). The runner_id is
      implicit in the client.
- [ ] `RunnerOut::send_connection_scoped`: hard-error / panic in
      debug; `Bye` is sent by `HttpLoop`'s shutdown path via
      `client.close_session()`.
- [ ] Replace shared `cloud_handle` in `Supervisor::run` with N
      per-runner `HttpLoop` tasks; drop the `out_tx`/`out_rx` mpsc.
- [ ] Drop the standalone `demux` task and the shared
      `mailboxes`/`status_sources`/`attach_runners` maps. Each
      `RunnerInstance` owns its own state.
- [ ] Drop `hello_emitter` / `attach_emitter` / `HelloRunnerMap` /
      `AttachRunnerMap`.
- [ ] Recovery on transient errors: exponential backoff ≤30s; on
      session-stale (network blip, **not** `409`), `open_session()`
      again. `409 session_evicted` is fatal for the
      `RunnerInstance`.
- [ ] On `RunnerLoop` exit (e.g. due to `RemoveRunner`),
      supervisor's join-handler triggers
      `client.close_session()` for that runner.
- [ ] **Ack-on-handle** (`design.md` decision #21,
      `daemon_module.md` §8): per-`RunnerInstance`
      `mpsc::UnboundedSender<AckEntry>` is plumbed from the
      `HttpLoop` into the `RunnerLoop`; `RunnerLoop` sends
      `stream_id` after the inner handler completes successfully.
      `HttpLoop::poll_once` drains all pending acks via
      `ack_rx.try_recv()`.
- [ ] Per-instance `InboundDedupe` (small bounded LRU keyed on
      `Envelope.message_id`, capacity ~256, TTL ~5 min) — when a
      redelivery from PEL arrives, ack-and-skip rather than
      re-running the handler.
- [ ] End-to-end integration test (single runner): assign → accept
      → run-event → completed over HTTP.
- [ ] Ack-on-handle integration test: poll returns 3 messages for
      the runner; block one handler; assert next poll's `ack` list
      only contains the two completed ids.

### 4c. Heartbeat → poll-body `status` folding

- [ ] New `RunnerStatusSource` thin wrapper around
      `state.rx_status` + `state.rx_in_flight` watch receivers.
      One per `RunnerInstance` (used by that runner's own
      `HttpLoop`, not a shared map).
- [ ] `HttpLoop::poll_once` snapshots the source → single `status`
      entry on the request body with current timestamp.
- [ ] Drop `hb_handles` Vec, the per-instance
      `tokio::time::interval` heartbeat tasks, and the `Heartbeat`
      ClientMsg dispatch path.
- [ ] Property: cloud-side `_reap_stale_busy_runs` continues to
      fire correctly under the new status flow.
- [ ] Multi-runner isolation integration test: simulate one runner
      going silent (no polls); confirm cloud flips it OFFLINE;
      sibling runner unaffected. Inject 5xx storm on runner A's
      poll; assert runner B's poll/refresh/event POSTs continue.

### 4d. `force_refresh` + per-runner refresh scheduler + protocol bump

- [ ] Add `ServerMsg::ForceRefresh { reason, min_rtg }` variant to
      `runner/src/cloud/protocol.rs`.
- [ ] `HttpLoop::dispatch_response` handles `ForceRefresh` inline
      via `client.force_refresh_inline()`; does **not** propagate
      to `RunnerLoop`.
- [ ] New per-runner `refresh_loop` task spawned by
      `Supervisor::run` (one alongside each `HttpLoop`); sleeps
      until `access_token.exp - 5min`, then calls `client.refresh()`.
- [ ] Bump `WIRE_VERSION` / `PROTOCOL_VERSION` to **4**; cloud-side
      `426 upgrade required` for v3 daemons.
- [ ] TUI / IPC `StatusSnapshot` surfaces per-runner `polling` /
      `refreshing` / `session_evicted` states.
- [ ] Runtime flag `PI_DASH_TRANSPORT=http|ws` (defaults to `http`
      after this sub-phase merges; `ws` allowed only during
      validation window).
- [ ] Integration test: queue a `force_refresh` server-side for one
      runner; daemon refreshes inline before next normal-cycle
      refresh. Sibling runner's refresh schedule unaffected.

## 5. Phase 5 — Cloud: retire WS as control plane

### 5.1 Code removal

- [ ] `send_to_runner` stops dual-writing — Redis stream only.
- [ ] Remove `receive_json` control-message hot path from the
      Channels consumer.
- [ ] Keep the consumer mounted **only** for the per-run upgrade
      ticket path (`design.md` §7.9); gate the handshake on the
      upgrade ticket.

### 5.2 Shared service extraction

- [ ] Extract `_apply_hello`, `_resolve_runner` (renamed from
      `_resolve_connection_runner`), group-add, online/offline
      transitions from `consumers.py` into a shared service module
      callable from both the WS consumer (per-run upgrade only) and
      the HTTP `POST /sessions/` endpoint.

### 5.3 Verification

- [ ] Monitoring dashboard: WS endpoint control traffic over a 24h
      window.
- [ ] Phase-5 done bar: zero control traffic on the WS endpoint;
      only upgrade-ticket handshakes.

## 6. Cross-cutting

### 6.1 Observability

- [ ] Metrics: per-runner poll latency (p50/p99), poll
      empty-vs-non-empty ratio, per-runner refresh count/rate,
      `force_refresh` queue depth, session evictions/min,
      `XAUTOCLAIM` reassignment count, PEL depth per active runner
      stream, offline-stream entries dropped, MachineToken usage
      (per-token last_used_at).
- [ ] Logs: every session lifecycle event per runner (`open`,
      `evict`, `delete`, `idle_timeout`); every refresh outcome
      with reason on failure.
- [ ] Alert: a runner attached but missing polls for >threshold —
      see `design.md` §7.7 for the threshold.

### 6.2 Documentation

- [ ] Update `runner/README.md` configure → service install → tui
      flow to reference the per-runner credentials format and the
      new `runners/<rid>/credentials.toml` layout.
- [ ] Note the `PI_DASH_TRANSPORT` runtime flag in operator docs
      (briefly; it's transitional).
- [ ] Document `pidash auth login` for CLI credential bootstrap on
      a machine that doesn't have a runner enrolled yet.

### 6.3 Security

- [ ] Confirm refresh-token-on-disk path is 0600 for every
      `runners/<rid>/credentials.toml` and
      `machine_token.toml`.
- [ ] Document the access-token-staleness window (≤ TTL) in
      operator-facing security notes.

### 6.4 Open questions to resolve before / during implementation

- [ ] `pidash connect --count N` CLI sugar for bulk runner
      enrollment (`design.md` §13). Out of scope for this
      milestone; future CLI work.
- [ ] Switch to Ed25519 access-token signing if a sidecar verifier
      appears (`design.md` §13).
- [ ] Refresh-rotation replay-window width — start at 1 generation;
      widen to 2 if spurious leak detections emerge in production.
- [ ] Per-run WS upgrade ticket lifetime — currently 60s; tighten
      or loosen after first real consumer.
- [ ] Mailbox backpressure under load — measure during phase-4
      integration testing; tune mpsc capacity if needed
      (`daemon_module.md` §14).
- [ ] MachineToken expiration policy — PAT-style with no TTL in v1;
      revisit if compliance requires.
- [ ] Single-flight refresh implementation choice
      (`tokio::sync::watch` vs `OnceCell`) — measure under
      multi-caller stress.

## Deferred / out of scope for v1

- Live log streaming via the per-run WS upgrade path (the canonical
  use case for `design.md` §7.9). Designed for, not built in v1.
- Multi-region cloud (single-Redis outbox; cross-region replication
  left for later).
- SSE / WebTransport push transports.
- Bulk runner enrollment endpoint
  (`POST /api/v1/runner/runners/enroll/?count=N`). Designed-for at
  the protocol level (each runner is independent), not built in v1.
- Runner workers as subprocesses. The per-runner credentials file
  layout makes this a future configuration change rather than a
  refactor; not built in v1.
