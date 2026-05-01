# Runner ↔ Cloud Transport: HTTPS Long-Poll + Per-Runner Tokens

> Directory: `.ai_design/move_to_https/`
>
> Successor to the always-on WebSocket transport currently in
> `runner/src/cloud/ws.rs` and `apps/api/pi_dash/runner/consumers.py`.
>
> Two patterns are borrowed, from two different sources:
>
> 1. **Per-runner identity — borrowed from GitHub Actions self-hosted
>    runners.** Each runner has its own credential material, its own
>    long-poll session, and its own listener token. There is no
>    higher-level "machine" or "connection" trust unit fronting the
>    runners. See `actions/runner` (`docs/design/auth.md`,
>    `Configuration/ConfigurationManager.cs`,
>    `Listener/MessageListener.cs`).
> 2. **Authentication shape — standard OAuth2 hygiene.** Short-TTL
>    access token + long-lived refresh credential, refresh-token
>    rotation with replay detection, refresh as the chokepoint that
>    re-evaluates workspace membership.
>
> The existing WebSocket protocol (`runner/src/cloud/protocol.rs`,
> Channels consumer) is **kept** as a future channel for data-heavy
> per-run streams (live log tail, large tool output, future media).
> It stops being the always-on connection.

## 1. Goal

- Eliminate the always-on stateful authenticated WebSocket as the
  control plane between cloud and daemon.
- **Drop the `Connection` abstraction.** The runner becomes a
  first-class trust unit: each runner row carries its own
  `created_by`, `workspace`, refresh-token hash, refresh-token
  generation, and revocation state. There is no separate "machine
  bond" between user and a fleet of runners; each runner is its own
  bond.
- Replace the always-on WS with **per-runner HTTPS long-poll**
  endpoints for control traffic and ordinary POSTs for runner→cloud
  upstream events. One runner = one session = one long-poll loop.
- Replace the long-lived `connection_secret` with a per-runner
  short-TTL access token + long-lived refresh credential. The
  refresh endpoint is the **chokepoint** that re-evaluates whether
  the runner's user is still a member of the runner's workspace,
  **at refresh time** (≤ access-token TTL of staleness for
  non-refresh requests; see §5.4).
- Preserve the multi-runner-per-machine architecture from
  `.ai_design/n_runners_in_same_machine/`: one daemon process
  supervises N `RunnerInstance`s on a machine, each with its own
  credentials, session, and poll loop. The daemon is a machine
  supervisor; **it does not own a shared cloud identity**.
- Preserve the existing WS protocol/code so future data-heavy
  per-run streams can opt into a one-shot WS upgrade without
  re-introducing always-on stateful auth.

Non-goals:

- Renaming the auto-issued credential for the `pidash` CLI. Today
  it's the `APIToken` minted at connection enrollment. After this
  change it's a separate, machine-scoped **`MachineToken`** model
  decoupled from runner enrollment (§5.6). Different threat model
  (interactive user CLI traffic), separately revocable, long-lived
  PAT-style credential.
- Changing the runner ↔ codex/claude-code subprocess protocol. Only
  the runner ↔ cloud edge moves.

## 1.1 Architectural layering

This migration is a transport replacement plus a trust-model
flattening. The runner ↔ cloud edge has three planes; only the
transport plane changes mechanically. The trust plane changes
shape (Connection → Runner) but the auth primitives stay the same.

1. **Message schema** — the `ClientMsg` / `ServerMsg` enums in
   `runner/src/cloud/protocol.rs` and their server-side counterparts.
   These stay as the canonical body schemas. Every data-bearing
   variant maps 1:1 to an HTTP body shape (§7.5 for daemon→cloud,
   §7.3 for cloud→daemon). No call site needs new types.
2. **Call-site API** — `RunnerOut::send(body: ClientMsg)` on the
   daemon, and the `on_run_started` / `on_run_event` /
   `on_approval_request` handlers on the cloud. Signatures unchanged.
   Internal dispatch reroutes to the new transport. See
   `daemon_module.md` §3 for the daemon-side dispatch table;
   `tasks.md` §3.3 + §5.2 for the cloud-side service extraction.
3. **Transport** — `runner/src/cloud/ws.rs` and the Channels consumer
   today; `runner/src/cloud/http.rs` and a set of DRF endpoints after
   Phase 4. **This plane is what gets replaced.** The WS code stays
   in the build for future opt-in per-run upgrade streams (decision
   #2, §7.9).

Four current variants — `Hello`, `Heartbeat`, `Bye`, `Ping` — are
absorbed into transport primitives rather than preserved as messages.
Their data still flows (session-open response body for `Hello`,
poll-request `status` field for `Heartbeat`, `DELETE` session for
`Bye`, long-poll's own server timeout for `Ping`) but they are no
longer `ClientMsg`/`ServerMsg` envelopes after Phase 4. One new
variant — `force_refresh` (decision #17) — is added at the schema
layer to support the new auth model; it rides the same poll path
as every other `ServerMsg`.

This layering is what makes the migration tractable: every
business-logic call site (run lifecycle, approvals, events)
recompiles unchanged; only the per-runner dispatcher behind
`RunnerOut` and the cloud-side handler plumbing change. It is also
what keeps the door open to bringing the WS transport back later for
a single per-run stream — the schema plane already supports it.

## 2. Why now

The current design has three structural problems, two already
identified in the codebase, the third surfaced during review of
`runner/src/cloud/ws.rs` and `consumers.py`:

1. **Long-lived bearer = no live authorization.** The
   `connection_secret` is bound at mint time to a `Connection` row
   with `created_by` (user) and `workspace`. After mint, no further
   re-check of `is_workspace_member(created_by, workspace)` happens.
   If the minting user is removed from the workspace, their daemon
   keeps working until somebody explicitly revokes the connection.
   Relying on a `post_delete` signal on `WorkspaceMember` is a
   mitigation, not a boundary.
2. **WS upgrade = one-time auth for a multi-hour session.** Even if
   per-request HTTP auth were live, the consumer authenticates once
   at WebSocket upgrade (`apps/api/pi_dash/runner/consumers.py:552-589`)
   and never re-validates. A nine-hour socket is a nine-hour blind
   spot.
3. **Stateful socket forces sticky load balancing.** Channels routes
   inbound frames to the consumer instance that holds the socket.
   Outbound `send_to_runner` already rides a Channels group, so this
   is partially solved, but the cloud still holds one open
   connection per machine that ties up asgi-worker resources for
   the daemon's lifetime. Rolling deploys drop every socket;
   daemons reconnect in a thundering herd.

The control plane is **not** a real-time streaming workload. Approvals
are human-in-the-loop and tolerate seconds of latency. Heartbeats
already run at 25-second intervals. Lifecycle events are discrete and
infrequent. The only event flow that is plausibly latency-sensitive
is `RunEvent` (per-tool-call/per-token output) — and that's exactly
the kind of "data-heavy" stream we want to keep the WS protocol
around to support, on demand, per run.

The merged trust model (Runner-as-trust-unit, no Connection layer)
is independent of the transport change but lands cleanly under the
same migration: with per-runner sessions, there's no asymmetry left
that justified a separate Connection layer in the first place.

## 3. Decisions locked in

| #   | Question                                                       | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| --- | -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ | ------ | ------------------------------------------------------------------------ | ------------- | ----------------------------------------------------------------------------------- |
| 1   | Replace WS as the always-on control plane?                     | Yes. Control traffic moves to per-runner HTTPS long-polling + per-request POSTs.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| 2   | Keep the WS protocol code?                                     | Yes. Reserved for **per-run, opt-in, time-bounded** data-heavy streams (live log tail, future media). No always-on socket. Authentication for a WS upgrade is a one-shot ticket minted by the access-token-bearing daemon (§7.9).                                                                                                                                                                                                                                                                                                                               |
| 3   | Token shape                                                    | **Per-runner.** Each runner has its own **refresh token** (long-lived, hashed in DB, on-disk 0600 in the runner's own credentials file) and its own **access token** (~1h TTL, self-contained signed token, daemon holds in memory only). The `Connection` table is **dropped**; its trust fields move onto `Runner` (§6).                                                                                                                                                                                                                                      |
| 4   | Where does workspace-membership authorization happen?          | At the per-runner refresh endpoint, on every refresh. Access-token verification is signature + `exp` (no DB hit) plus a single indexed point query on `Runner` to read `refresh_token_generation` for the `rtg` lower-bound check. No live workspace-membership check on the request hot path. Authorization staleness is therefore bounded by access-token TTL (≤1h by default). Sensitive endpoints may opt into per-request live re-check (§5.4).                                                                                                            |
| 5   | What happens when refresh is denied because membership lapsed? | **Lazy revoke-on-deny, per-runner.** The refresh endpoint calls `Runner.revoke()` on the failing path. That cascades to that runner's in-flight `AgentRun` (cancelled), its pinned QUEUED runs (lose pin), and re-drains the runner's pod so siblings can pick up orphaned work. No separate sweeper job needed; eventual consistency is bounded by the access-token TTL. When a user is kicked from a workspace, every runner owned by them in that workspace fails its next refresh and self-revokes — N revokes instead of one Connection revoke.            |
| 6   | Refresh-token rotation                                         | Rotate on every successful refresh. Store the previous token's hash in `Runner.previous_refresh_token_hash` (single-slot history). Lookup at refresh time tries `refresh_token_hash` first; if that misses but `previous_refresh_token_hash` matches, treat as a leak — call `Runner.revoke()`.                                                                                                                                                                                                                                                                 |
| 7   | Heartbeat / per-runner liveness                                | Each long-poll request body carries that runner's `status` entry `{status, in_flight_run, ts}`. The server applies it: updates `Runner.last_heartbeat_at`, runs `_reap_stale_busy_runs`. Because each poll is for **one** runner, the body has one status entry, not an array. The dedicated `Heartbeat` ClientMsg goes away.                                                                                                                                                                                                                                   |
| 8   | Outbox backing store                                           | **Redis Streams**, **one persistent stream per runner** (`runner_stream:{rid}`), **one persistent consumer group per runner** (`runner-group:{rid}`), and **one consumer name per session** (`consumer-{sid}`). Every control-plane message in the stream carries `type` and `mid`. `XREADGROUP` against `consumer-{sid}` does **not consume**; entries remain in the PEL until `XACK`. On session eviction, the new session `XAUTOCLAIM`s the prior consumer name's PEL onto the new consumer name **within the same stream and group** (paginated). See §7.4. |
| 9   | Ordering, dedupe, and ack model                                | Stream IDs are monotonic within `runner_stream:{rid}`. Ack body is the **explicit flat list** `["<stream_id_1>", ...]`. Server issues `XACK runner_stream:{rid} runner-group:{rid} <id1> [<id2> ...]` (XACK takes exact IDs). The `Envelope.message_id` (mid) stays as an application-level dedupe key so PEL redelivery after a daemon crash isn't double-handled.                                                                                                                                                                                             |
| 10  | Per-runner session model                                       | One runner = one session = one long-poll loop. Session is created with `POST /runners/<rid>/sessions/`, which carries the metadata today's per-runner `Hello` carries (`version`, `os`, `arch`, `status`, `in_flight_run`, `project_slug`) and runs the existing `_apply_hello` + group-add + online-mark + drain logic in one step. **There is no separate `attach/` endpoint**, because there is no connection-level session into which runners attach.                                                                                                       |
| 11  | Channel for `RunEvent`                                         | Batched POST `/api/v1/runner/runs/<run_id>/events/`, body `{"events": [...]}`. Daemon batches by time (≤ 250ms) **or** size (≤ 64 KB), whichever fires first. Phase 5+ may upgrade per-run to a WS stream for runs flagged data-heavy.                                                                                                                                                                                                                                                                                                                          |
| 12  | CLI credential                                                 | **`MachineToken`** (separate model, machine-scoped). Auto-issued on the first runner enrollment for the same `(user, host_label)` pair if no live token exists. Independently revocable. Distinct threat model (interactive user CLI traffic) and bypassed by runner transport. See §5.6.                                                                                                                                                                                                                                                                       |
| 13  | Pre-existing daemons                                           | None in production. The protocol described here is the only one shipped. The cloud serves the new endpoints from day one; the WS endpoint stays mounted but is no longer dialed by the daemon's main loop. Step-down of the WS endpoint from the control plane happens with the protocol-version bump in Phase 4. **No data migration required.**                                                                                                                                                                                                               |
| 14  | Protocol version                                               | Bump the cloud-acknowledged `protocol_version` to **4**. The bump signals "control plane is HTTP, per-runner; WS is opt-in per-run." Older daemons advertising version 3 are rejected with a clear error pointing at the upgrade path.                                                                                                                                                                                                                                                                                                                          |
| 15  | TTLs                                                           | Access token: **1 hour**. Refresh token: **no fixed expiry** (revocable; tied to the Runner row). Long-poll timeout: **25 seconds** server-side. Daemon recovers by re-polling immediately on empty response.                                                                                                                                                                                                                                                                                                                                                   |
| 16  | Session fencing                                                | Each runner has at most **one active session**. `POST /runners/<rid>/sessions/` evicts any prior session **for that runner only** — sibling runners on the same machine are unaffected. The prior session is marked revoked; its in-flight long-poll returns `409 session_evicted`. PEL handoff happens at session-open time via paginated `XAUTOCLAIM` within `runner_stream:{rid}`. `session_id` is a required URL **path** segment on every poll/ack call; mismatched session_id is rejected with `409 session_evicted`.                                     |
| 17  | Server-driven force refresh                                    | Per-runner. `RunnerForceRefresh(runner, min_rtg, reason)` table; `force_refresh` ServerMsg arrives via that runner's poll. Use cases: signing-key rotation (bulk insert N rows), suspected leak (one row), admin-initiated re-authz.                                                                                                                                                                                                                                                                                                                            |
| 18  | Offline enqueue policy                                         | Per-runner offline stream (`runner_offline_stream:{rid}`, MAXLEN 1000, 24h TTL). `assign                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | cancel | decide | resume_ack`rejected at scheduling time if runner is offline;`config_push | remove_runner | revoke` may queue. On session-open, the offline stream drains into the live stream. |
| 19  | Upstream idempotency store                                     | v1 uses `RunMessageDedupe(run, message_id, created_at)` with a unique constraint on `(run, message_id)`. Periodic cleanup deletes rows older than 7 days.                                                                                                                                                                                                                                                                                                                                                                                                       |
| 20  | Rate limiting                                                  | Per-runner scoped throttles. Poll: protocol-bounded (one in-flight per session, ~25s server timeout). Upstream POSTs: `RunnerRateThrottle` keyed by `runner_id`, sized for event batches.                                                                                                                                                                                                                                                                                                                                                                       |
| 21  | Delivery semantics — ack-on-handle                             | Per-runner ack-on-handle. Stream id enters the next poll's `ack` list **only after** the per-runner handler in `RunnerLoop` has finished processing. Plumbing: per-runner `mpsc::UnboundedSender<AckEntry>` from `RunnerLoop` to that runner's `HttpLoop`. PEL on session restart re-delivers; per-instance `mid` LRU dedupes.                                                                                                                                                                                                                                  |

## 4. Conceptual model

| Concept                | What it is                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Runner**             | First-class trust + worker entity. Owns `created_by` (user), `workspace`, `pod`, `refresh_token_hash`, `refresh_token_generation`, `previous_refresh_token_hash`, `revoked_at`, plus the existing operational fields (`status`, `last_heartbeat_at`, `host_label`, `agent_versions`). The unit of trust the refresh token authenticates against. Replaces the old `Runner` + `Connection` two-table chain with a single self-sufficient row.                                                                                                                                |
| **Refresh token**      | Per-runner long-lived credential. Daemon stores at `~/.config/apple-pi-dash-runner/runners/<rid>/credentials.toml` (0600). Single-use rotation: each refresh consumes it and returns the next. Authenticates **against the user's current workspace state**, not a snapshot.                                                                                                                                                                                                                                                                                                |
| **Access token**       | Per-runner short-TTL signed token (HS256 by default; Ed25519 reserved). Self-contained: payload includes `runner_id` (sub), `user_id`, `workspace_id`, `iat`, `exp`, `rtg`, `kid`. Verified statelessly on every API request.                                                                                                                                                                                                                                                                                                                                               |
| **MachineToken**       | Separate, lightweight CLI credential. Long-lived PAT bound to `(user, workspace, host_label)`. Used by `pidash` CLI to call `/api/v1/` user-action endpoints (issues, comments, state changes). Independently revocable. Auto-issued on the first runner enrollment per machine; reissuable via `pidash auth login`. **Not** consulted by the runner transport. See §5.6.                                                                                                                                                                                                   |
| **RunnerSession**      | Server-side row that owns delivery for one runner. Created by `POST /runners/<rid>/sessions/`; one active session per runner (newer evicts older, §7.6). Identified by `session_id` carried on every poll/ack URL. The session is the consumer identity that owns the runner stream's PEL.                                                                                                                                                                                                                                                                                  |
| **Long-poll**          | Each runner's only persistent activity. One open `POST /runners/<rid>/sessions/<sid>/poll` per session, ≤25s server timeout, returns 0..N pending control messages from `runner_stream:{rid}`. Request body carries that runner's status entry and a flat ack list (stream ids the daemon has finished handling since the last poll).                                                                                                                                                                                                                                       |
| **Outbox**             | Redis Streams, **one stream per runner** (`runner_stream:{rid}`, persistent), **one consumer group per runner** (`runner-group:{rid}`, persistent), **per-session consumer name** (`consumer-{sid}`). `enqueue_for_runner` → `XADD` (no inline `MAXLEN` — see §7.4 retention contract); long-poll → `XREADGROUP BLOCK` against `consumer-{sid}` (no-consume; PEL retains until XACK); ack → `XACK <stream> <group> <id1> [<id2> ...]`. On session evict, paginated `XAUTOCLAIM` reassigns `consumer-{old_sid}`'s PEL to `consumer-{new_sid}` — within stream, within group. |
| **WS (legacy/opt-in)** | Existing `runner/src/cloud/ws.rs` + Channels consumer. Reachable only via the per-run upgrade endpoint (§7.9). Not used by the daemon's connection loop after Phase 4.                                                                                                                                                                                                                                                                                                                                                                                                      |

## 5. Authentication

### 5.1 Token issuance

`POST /api/v1/runner/runners/enroll/` (formerly `connections/enroll/`,
moved to reflect the runner-centric model):

- Consumes a one-time enrollment token (current shape).
- Creates a new `Runner` row bound to the enrolling user's
  workspace; the enrollment token's project context determines which
  Pod the runner joins.
- Returns the runner's **refresh token** + **access token** (no
  longer a single `connection_secret`).
- `Runner.refresh_token_generation` (new column, default 0) is
  incremented to 1.
- `Runner.previous_refresh_token_hash` is `""` on first issuance.
- **MachineToken bootstrap**: if the calling user has no live
  `MachineToken` row for this machine's `host_label`, the cloud
  also mints one and returns it in the response. Otherwise this
  field is omitted. (§5.6.)

Response shape:

```json
{
  "runner_id": "...",
  "runner_name": "...",
  "refresh_token": "rt_...",
  "access_token": "at_...",
  "access_token_expires_at": "...",
  "refresh_token_generation": 1,
  "workspace_slug": "...",
  "pod_slug": "...",
  "long_poll_interval_secs": 25,
  "protocol_version": 4,
  "machine_token": "mt_..." // only on first per-machine enrollment
}
```

### 5.2 Access-token format

Self-contained signed payload. No DB lookup on the hot path beyond
a single indexed point query on `Runner` for the rtg lower bound.

**Decision**: HS256 for v1 (rotating server-side key ring). Switch
to Ed25519 only if a non-Django verifier becomes a real requirement.

**Key storage / rotation contract:**

- Signing keys live in Django settings as an **ordered key ring**:
  `RUNNER_ACCESS_TOKEN_KEYS = [{"kid": "2026-04-1", "secret": "...",
"status": "active"}, {"kid": "2026-02-1", "secret": "...",
"status": "verify_only"}]`.
- Exactly one key is `active` for minting. Any number may be
  `verify_only` during rotation overlap.
- The daemon does **not** need key material; it never verifies
  access tokens locally. It treats them as opaque bearer tokens.
- Rotation procedure:
  1. Add new key as `active`, demote old `active` to `verify_only`.
  2. Bulk-insert `RunnerForceRefresh` rows for every active runner.
  3. After `access_token_ttl_secs + safety_margin`, remove the old
     key from the ring.

Payload:

```json
{
  "kid": "2026-04-1",
  "iss": "pi-dash-cloud",
  "sub": "<runner_id>",
  "uid": "<user_id>",
  "wid": "<workspace_id>",
  "iat": 1714080000,
  "exp": 1714083600,
  "rtg": 1
}
```

`rtg` is the runner's refresh-token generation at the moment this
access token was minted. The cloud rejects access tokens whose
`rtg` is older than `runner.refresh_token_generation - 1`
(one-generation grace handles in-flight requests during rotation).

Verification order:

1. Verify signature by `kid`; reject unknown `kid`.
2. Verify `exp`.
3. Verify `rtg` against the lower bound on the `Runner` row.
4. If a queued `RunnerForceRefresh.min_rtg` exists for this runner,
   reject tokens with `rtg < min_rtg` even if they pass step 3.

### 5.3 Refresh endpoint

`POST /api/v1/runner/runners/<runner_id>/refresh/`

Authentication: bearer the **refresh token** in the `Authorization`
header. No access token required.

Logic, in order, in a single transaction with `select_for_update` on
the `Runner` row:

1. Look up `Runner` by `id=runner_id`. Not found → 401
   `invalid_refresh_token`.
2. If `runner.revoked_at IS NOT NULL` → 401 `runner_revoked`.
3. **Token-hash match decision**:
   - `hash(presented) == runner.refresh_token_hash` → happy path.
   - Else if `runner.previous_refresh_token_hash != ""` and
     `hash(presented) == runner.previous_refresh_token_hash` →
     **leak detected**. Call `runner.revoke()`. Return 401
     `refresh_token_replayed`.
   - Else → 401 `invalid_refresh_token`.
4. Live check: `is_workspace_member(runner.created_by, runner.workspace_id)`.
   False → **lazy revoke**. Call `runner.revoke()`. Return 401
   `membership_revoked`.
5. Mint new tokens. Atomically:
   - Move current `refresh_token_hash` → `previous_refresh_token_hash`.
   - Set `refresh_token_hash` to the new token's hash.
   - Increment `refresh_token_generation`.
   - Mint new access token with the new `rtg`.
6. Delete any `RunnerForceRefresh` row for this runner.
7. Return both tokens + new generation.

The daemon writes the new refresh token to disk **before** discarding
the old one in memory. Crash-window analysis matches the previous
design: a crash between server step 7 and the daemon's disk-write
strands the daemon with the old token; on retry, the server detects
the previous-hash match and revokes the runner. Recovery is to
re-enroll that runner.

### 5.4 Authentication for every other runner-transport endpoint

A single DRF authentication class — `RunnerAccessTokenAuthentication` —
handles every transport endpoint except `/refresh/` and `/enroll/`.
Behavior:

- Pull `Authorization: Bearer <jwt>` from the request.
- Verify signature and `exp`. On failure → 401 `access_token_expired`
  (so the daemon knows to refresh, not re-enroll).
- Verify `rtg` against `runner.refresh_token_generation - 1` lower
  bound. The runner lookup is a single indexed point query.
- For runner-scoped endpoints with `<runner_id>` in the URL: assert
  the URL `runner_id` matches `token.sub`; reject 403
  `runner_id_mismatch` if not.
- For run-scoped endpoints (`/runs/<run_id>/...`): resolve the run,
  assert `run.runner_id == token.sub`; reject 403
  `run_not_owned_by_runner` otherwise.
- Set `request.auth_runner` for downstream handlers.

**Naming this honestly**: this is **not** "live authorization" in the
sense GitHub uses. This is **authorization re-evaluated on each
refresh cycle**, with up to access-token-TTL staleness on every
other request. The cloud may shorten the staleness window by
queuing a `RunnerForceRefresh` row + emitting `force_refresh` on
the runner's poll (decision #17, §7.8) — typically reducing it to
seconds, not the full TTL.

### 5.5 What lives where on disk

The daemon's credentials directory:

```
~/.config/apple-pi-dash-runner/
  daemon.toml                           # host_label, daemon-level config
  machine_token.toml                    # MachineToken for `pidash` CLI
                                        # 0600, owned by the user
  runners/
    <rid_A>/
      credentials.toml                  # runner A's refresh state, 0600
    <rid_B>/
      credentials.toml                  # runner B's, 0600
```

`runners/<rid>/credentials.toml`:

```toml
[runner]
id = "..."
name = "..."

[refresh]
token = "rt_..."
generation = 7
issued_at = "..."
```

`machine_token.toml`:

```toml
[machine_token]
token = "mt_..."
issued_at = "..."
host_label = "..."
```

The access token never touches disk.

**Ownership and concurrency:** each `runners/<rid>/credentials.toml`
is owned by exactly one `RunnerInstance`. Writes are atomic
(write-temp + rename); concurrent runner refreshes do not contend.
The daemon process supervises but does not read or write the
per-runner files itself, which keeps subprocess-isolation a
configuration choice rather than a refactor.

### 5.6 MachineToken (CLI credential)

The auto-issued credential the `pidash` CLI presents when calling
`/api/v1/` user-action endpoints (issues, comments, state changes).
Distinct from runner transport credentials.

**Model** — new table:

| Column              | Type                       | Notes                                                 |
| ------------------- | -------------------------- | ----------------------------------------------------- |
| `id`                | UUIDField (PK)             |                                                       |
| `user`              | FK → User                  | Whose CLI traffic this represents                     |
| `workspace`         | FK → Workspace             | Workspace this token grants access to                 |
| `host_label`        | CharField(255)             | Machine identifier the daemon reports at enrollment   |
| `token_hash`        | indexed hash               | The bearer credential, hashed                         |
| `token_fingerprint` | char                       | Last-N chars for revoke UX                            |
| `label`             | CharField                  | Auto-generated `"machine: <host_label>"`              |
| `created_at`        | DateTimeField              | `auto_now_add`                                        |
| `revoked_at`        | DateTimeField (null)       | Independent revocation                                |
| `last_used_at`      | DateTimeField (null)       | For staleness reporting in the web UI                 |
| `is_service`        | BooleanField, default true | Routes through the 300/min `ServiceTokenRateThrottle` |

Constraint: at most one non-revoked token per `(user, workspace,
host_label)`.

**Auth class** — `MachineTokenAuthentication` (DRF):

- Bearer token, hash-match on `token_hash`.
- 401 `machine_token_invalid` on miss; 401 `machine_token_revoked`
  on revoked row.
- **Per-request `is_workspace_member` check** (since there's no
  refresh chokepoint here — these are PATs). Membership lapse →
  set `revoked_at`, return 401 `membership_revoked`.
- Updates `last_used_at` (best-effort, not in transaction).

**Minting policy:**

- **Bootstrap on first runner enrollment per machine**: the
  enrollment endpoint checks for a live MachineToken matching
  `(user, workspace, host_label)`. If none, it mints one and
  includes it in the enrollment response (§5.1). If one exists,
  the response omits the field — the daemon's existing
  `machine_token.toml` is still valid.
- **Explicit `pidash auth login`** mints a MachineToken via a
  separate user-auth flow (web-UI-issued one-time ticket). Used
  when the operator wants to re-key the CLI without re-enrolling
  any runner.
- **Web UI**: list/revoke MachineTokens for the current user per
  workspace.

**Disk:** `~/.config/apple-pi-dash-runner/machine_token.toml`
(0600). Independent of any runner; survives runner enrollment and
revocation.

## 6. Data model

### Runner — gains the trust fields formerly on Connection

Additive changes to `apps/api/pi_dash/runner/models.py`'s `Runner`:

- `created_by`: FK → User (was on Connection).
- `workspace`: FK → Workspace, denormalized from
  `runner.pod.project.workspace` for fast revocation queries.
- `refresh_token_hash`: indexed CharField(128). Hash of the current
  refresh token.
- `refresh_token_fingerprint`: CharField(8). Last-N chars for UX.
- `refresh_token_generation`: PositiveIntegerField, default 0.
  Bumped on every successful refresh.
- `previous_refresh_token_hash`: CharField(128, blank=True,
  default=""). Single-slot history for replay detection (§5.3).
- `access_token_signing_key_version`: PositiveIntegerField,
  default 1. Reserved; not used in v1.
- `revoked_at`: DateTimeField (null) — consolidated revocation flag.
- `revoked_reason`: CharField (32, blank). Values include
  `manual_revoke`, `membership_revoked`, `refresh_token_replayed`,
  `runner_removed`.
- `enrolled_at`: DateTimeField, `auto_now_add`.

The existing operational fields (`status`, `last_heartbeat_at`,
`host_label`, `agent_versions`, `pod`) stay as today.

### Connection table — DROPPED

The `Connection` table is removed entirely. There is no migration
because there is no production data (decision #13). Code paths
that referenced `Connection` move to `Runner` directly. The
`auto_now_add` enrollment metadata that used to live on Connection
(e.g. `enrolled_at`, `host_label`) moves onto `Runner`.

### RunnerSession — per-runner

Renamed conceptually but the table shape is similar:

| Column             | Type                  | Notes                                                          |
| ------------------ | --------------------- | -------------------------------------------------------------- |
| `id`               | UUIDField (PK)        | `session_id` carried on every poll URL                         |
| `runner`           | FK → Runner           | `on_delete=CASCADE`                                            |
| `created_at`       | DateTimeField         | `auto_now_add`                                                 |
| `last_seen_at`     | DateTimeField         | Updated on every poll request                                  |
| `revoked_at`       | DateTimeField (null)  | Set on eviction or session-DELETE                              |
| `revoked_reason`   | CharField (32, blank) | `superseded`, `client_close`, `idle_timeout`, `runner_revoked` |
| `protocol_version` | PositiveIntegerField  | Version negotiated at session-open                             |

Constraint: only one active session per runner
(`UniqueConstraint(fields=["runner"], condition=Q(revoked_at__isnull=True))`).

Session-open metadata (`host_label`, `agent_versions`) is reported
to the runner row, not stored on the session. The session row is
purely "this runner's current delivery ownership".

### RunnerForceRefresh — per-runner

| Column       | Type                 | Notes                                                     |
| ------------ | -------------------- | --------------------------------------------------------- |
| `runner`     | OneToOne → Runner    | one outstanding force-refresh floor per runner            |
| `min_rtg`    | PositiveIntegerField | optional stronger lower bound than normal grace           |
| `reason`     | CharField(32)        | `signing_key_rotation`, `admin_request`, `suspected_leak` |
| `created_at` | DateTimeField        |                                                           |

### RunMessageDedupe — unchanged

| Column       | Type              | Notes                                     |
| ------------ | ----------------- | ----------------------------------------- |
| `id`         | BigAutoField (PK) |                                           |
| `run`        | FK → AgentRun     | `on_delete=CASCADE`                       |
| `message_id` | CharField(64)     | Runner-supplied `mid` / `Idempotency-Key` |
| `created_at` | DateTimeField     | `auto_now_add`                            |

Constraint: `UniqueConstraint(fields=["run", "message_id"])`.

### MachineToken — see §5.6

### Pod / Project / Workspace — unchanged

The existing `Pod ↔ Project ↔ Workspace` chain is unchanged. `Pod`
remains a logical scheduling/grouping concept. `Runner.pod` FK
stays. `Pod` itself does not gain `created_by`; the project owner
is the natural owner of the pod, and individual runners on the pod
carry their own `created_by` because each runner was enrolled by a
specific user.

### Migration

Single migration since there is no production data (decision #13):

1. Add the new columns to `Runner`.
2. Drop the `Connection` table.
3. Rekey `RunnerSession` to `runner`-scoped (drop the connection
   FK, add runner FK, switch the unique constraint).
4. Rekey `RunnerForceRefresh` to `runner`-scoped.
5. Add `MachineToken` table.

## 7. Wire protocol mapping

Every current `ClientMsg` / `ServerMsg` variant maps to an HTTP
endpoint scoped to one runner. The WS protocol enums in
`runner/src/cloud/protocol.rs` stay as the **body schemas** so the
runner code re-uses serialization unchanged.

The model is **runner-bound**. There are no connection-scoped
messages; every frame either rides one runner's stream or applies
to one runner's transport. The multi-runner-per-machine semantics
preserved here are at the **process** level (one daemon supervises
N runners) — the cloud sees N independent runner identities, each
with its own session, stream, and poll.

### 7.1 Per-runner session lifecycle (replaces connection-level Hello + per-runner Hello)

```
POST /api/v1/runner/runners/<rid>/sessions/
Authorization: Bearer <access_token>   # token.sub must equal <rid>
Body: {
  "version": "...",
  "os": "...",
  "arch": "...",
  "status": "idle" | "busy",
  "in_flight_run": null | "<uuid>",
  "project_slug": "...",       // optional, validated against runner.pod.project
  "host_label": "...",
  "agent_versions": { ... }
}
→ 201 {
    "session_id": "...",
    "server_time": "...",
    "long_poll_interval_secs": 25,
    "protocol_version": 4,
    "welcome": { ... existing per-runner Welcome payload ... },
    "resume_ack": { ... } | null    // present iff in_flight_run was set
  }
```

Server behavior — combines today's session-open + per-runner attach
into one step:

1. Verify access token; resolve `Runner` (assert `token.sub == rid`).
2. **Evict any prior active session** for this runner: mark
   `revoked_at` on the old session, publish
   `session_eviction:<rid>` pub/sub with `{old_sid, new_sid}`. The
   prior session's in-flight long-poll receives the pub/sub and
   returns `409 session_evicted`. Decision #16.
3. **Ensure the persistent stream and group exist** (idempotent):
   ```
   XGROUP CREATE runner_stream:{rid} runner-group:{rid} $ MKSTREAM
   ```
   Ignore `BUSYGROUP` in steady state.
4. **Claim the prior consumer's PEL onto the new consumer name** via
   paginated `XAUTOCLAIM` (within stream, within group):
   ```
   cursor = "0-0"
   loop:
       reply = XAUTOCLAIM runner_stream:{rid} runner-group:{rid}
                          consumer-{new_sid}
                          min-idle-time=0
                          start=<cursor>
                          COUNT 1000
                          JUSTID
       cursor = reply.next_cursor
       if cursor == "0-0": break
   ```
5. Validate `project_slug` matches `runner.pod.project.identifier`
   if provided (current `_resolve_connection_runner` logic, renamed).
6. Apply `_apply_hello` (metadata save + stale-busy reaping with
   `in_flight_run`).
7. Mark runner `ONLINE`.
8. Drain any queued runs (`drain_for_runner_by_id`).
9. **Drain the offline buffer into the live stream**:
   ```
   XRANGE runner_offline_stream:{rid} - +
   → for each entry: XADD runner_stream:{rid} fields:{...msg, offline_id: <orig_id>}
   → XDEL runner_offline_stream:{rid} <orig_id>
   ```
   Bounded by §7.4's offline cap.
10. If `in_flight_run` was set, kick off `_resume_run` and include a
    `resume_ack` body in the response.
11. Create the `RunnerSession` row.
12. Return Welcome (and optional `resume_ack`).

```
DELETE /api/v1/runner/runners/<rid>/sessions/<sid>/
```

Clean shutdown for this runner. Server reaps the session row and the
consumer-name ownership; the persistent stream/group survive. If
the daemon disappears without calling DELETE, the session is reaped
after `2 × long_poll_interval_secs` of no poll activity.

### 7.2 No separate attach endpoint

In the per-runner architecture, opening a session for a runner **is**
attaching that runner. There is no second-step attach call. The
metadata that today's per-runner `Hello` carries is in the
session-open body; the existing `_apply_hello` + group-add +
online-mark + drain + resume logic runs synchronously inside
`POST /sessions/`.

Detach is symmetric to session close:

```
DELETE /api/v1/runner/runners/<rid>/sessions/<sid>/
```

— ends this runner's session and marks it offline. The runner row
itself survives; a future session-open revives it.

### 7.3 Long-poll (replaces cloud→daemon `ServerMsg` push)

```
POST /api/v1/runner/runners/<rid>/sessions/<sid>/poll
Authorization: Bearer <access_token>   # token.sub must equal <rid>
Body — POST not GET so the request body can carry ack + status:
{
  "ack": ["<stream_id_1>", "<stream_id_2>"],
  "status": {
    "status": "idle" | "busy",
    "in_flight_run": null | "<uuid>",
    "ts": "..."
  }
}
```

`ack` is the explicit flat list of stream ids the daemon has finished
handling since the last poll (decision #21). `status` is a single
entry — the poll is for one runner. Empty ack list is fine.

`session_id` is mandatory; stale session_id → `409 session_evicted`
and the daemon shuts down this runner's loop (it's been displaced —
see decision #16).

Server side, in this order:

1. Verify session is active for this runner; reject with
   `409 session_evicted` if not.
2. Update `RunnerSession.last_seen_at`.
3. Apply `status`: update `Runner.last_heartbeat_at`, run
   `_reap_stale_busy_runs(runner, status)`.
4. If `ack` is non-empty:
   `XACK runner_stream:{rid} runner-group:{rid} <id1> [<id2> ...]`.
5. Issue one `XREADGROUP`. The first poll after `POST /sessions/`
   uses `0` (re-fetch this consumer's PEL — newly-claimed entries
   from the prior session):
   ```
   XREADGROUP GROUP runner-group:{rid} consumer-{sid}
              COUNT 100 BLOCK 25000
              STREAMS runner_stream:{rid}
                      0
   ```
   Subsequent polls use `>` once the PEL has drained:
   ```
   XREADGROUP GROUP runner-group:{rid} consumer-{sid}
              COUNT 100 BLOCK 25000
              STREAMS runner_stream:{rid}
                      >
   ```
   Per-session id-marker selection tracked in
   `session_pel_drained:{sid}` (Redis key); set after a `0`-based
   read returns empty.
6. Return drained entries with `stream_id`, `mid`, `type`, `body`.

Response body:

```json
{
  "messages": [
    {
      "stream_id": "1714080000-0",
      "mid": "...",
      "type": "assign",
      "body": { ... existing ServerMsg body ... }
    }
  ],
  "server_time": "...",
  "long_poll_interval_secs": 25
}
```

ServerMsg-type mapping (`messages[i].type`):

| Current frame         | `messages[i].type`                                                              |
| --------------------- | ------------------------------------------------------------------------------- |
| `Welcome`             | Returned synchronously from `POST /sessions/`, not via poll.                    |
| `Assign`              | `assign`                                                                        |
| `Cancel`              | `cancel`                                                                        |
| `Decide`              | `decide`                                                                        |
| `ConfigPush`          | `config_push`                                                                   |
| `Ping`                | (gone; long-poll itself replaces it)                                            |
| `Revoke`              | `revoke` (now per-runner — applies to this runner only)                         |
| `RemoveRunner`        | `remove_runner`                                                                 |
| `ResumeAck`           | `resume_ack` (also returned synchronously from `POST /sessions/` if applicable) |
| (new) `force_refresh` | `force_refresh` (decision #17)                                                  |

There are no connection-scoped messages — every frame is for the
runner that owns this poll.

### 7.4 Outbox semantics (Redis Streams + per-runner consumer group)

Backing store: **Redis Streams**, with three keying levels (matching
decision #8):

- **Stream**: one per runner, keyed `runner_stream:{rid}`.
  **Persistent across sessions** — created on first session open
  and never destroyed for the runner's lifetime (subject to
  PEL-aware sweeper-driven trimming; see retention contract below).
- **Consumer group**: one per runner, named `runner-group:{rid}`.
  Persistent across sessions. The group's PEL is the authoritative
  "delivered but not yet acked" record across the runner's history.
- **Consumer name**: per session, named `consumer-{sid}`. Changes on
  session eviction. Each consumer name has its own PEL slice within
  the group.

Session handoff stays within stream/group: `XAUTOCLAIM` reassigns
ownership between consumer names at `POST /sessions/` time (§7.1).

Implementation of `enqueue_for_runner`:

```python
def enqueue_for_runner(runner_id, msg):
    sid = active_session_id_for_runner(runner_id)
    if sid is None:
        # Offline policy (decision #18):
        if msg["type"] in {"assign", "cancel", "decide", "resume_ack"}:
            raise RunnerOfflineError(runner_id)
        stream_id = redis.xadd(
            f"runner_offline_stream:{runner_id}",
            msg,
            maxlen=1000,
            approximate=True,
        )
        redis.expire(f"runner_offline_stream:{runner_id}", 86400)
        return stream_id
    # Live session: append to the runner's stream. NOTE: no inline
    # MAXLEN trim — see retention contract below. Trimming is
    # sweeper-driven and PEL-aware to preserve at-least-once.
    return redis.xadd(f"runner_stream:{runner_id}", msg)
```

**Ack semantics:**

- `XREADGROUP <group> <consumer> ... STREAMS <stream> >` delivers
  only entries not yet seen by any consumer in the group; entries
  enter that consumer's PEL.
- `XREADGROUP <group> <consumer> ... STREAMS <stream> 0` re-fetches
  the consumer's PEL — used by the first poll after session-open.
- `XACK <stream> <group> <id1> [<id2> ...]` removes specific IDs.
  XACK is exact-id, not range — the daemon's `ack` body field
  carries the explicit flat list.
- If the daemon crashes mid-handle, those ids are still in the PEL.
  On next session-open (new `session_id`), §7.1 reassigns them onto
  `consumer-{new_sid}` via paginated `XAUTOCLAIM`. Application-level
  `mid` dedupe at the daemon side gates double-handling.

The PEL is the durable record of "delivered but not yet acked"
messages.

Retention / cleanup contract:

- **No inline `MAXLEN` trim on `runner_stream:{rid}`.** Redis ≤7.3
  trims by `XADD ... MAXLEN`/`XTRIM` unconditionally — entries
  referenced in a consumer-group's PEL are evicted from the stream
  even though the PEL still references them. After such a trim, the
  PEL's IDs survive but their payloads are gone, and a subsequent
  `XREADGROUP ... 0` re-fetch returns the IDs with `nil` bodies.
  That breaks at-least-once redelivery. (Redis 7.4 added an
  `ACKED` flag to address this, but pi-dash targets Redis 6.2.7
  per `CLAUDE.md`, so we avoid that primitive.) Trimming is
  **sweeper-driven and PEL-aware** — see `sweep_old_streams` in
  §7.10.
- On session eviction or session-DELETE, the **stream itself is not
  destroyed** — `runner_stream:{rid}` and `runner-group:{rid}` are
  persistent. What changes is the active consumer name. The old
  consumer name's PEL is retained until either:
  - The next session-open `XAUTOCLAIM` reassigns it onto the new
    consumer (§7.1), or
  - `sweep_old_streams` (§7.10) deletes idle consumer names that
    haven't been claimed within `2 × access_token_ttl_secs` of
    session eviction.
- Runner streams whose runner is revoked or has been idle with
  `XLEN == 0` for >24h are eligible for deletion by
  `sweep_old_streams`.
- `RunMessageDedupe` rows older than 7 days are deleted by periodic
  cleanup.

### 7.5 Daemon → cloud (replaces ClientMsg events)

| Current frame       | New endpoint                                             | Notes                                            |
| ------------------- | -------------------------------------------------------- | ------------------------------------------------ |
| `Hello`             | `POST /api/v1/runner/runners/<rid>/sessions/` (§7.1)     | Combined with session-open                       |
| `Heartbeat`         | gone — folded into the long-poll request body's `status` | per-runner, drives stale-busy reaping            |
| `Accept`            | `POST /api/v1/runner/runs/<run_id>/accept/`              | Body carries `workspace_state`.                  |
| `RunStarted`        | `POST /api/v1/runner/runs/<run_id>/started/`             |                                                  |
| `RunEvent`          | `POST /api/v1/runner/runs/<run_id>/events/` (batched)    | Body `{"events": [{seq, kind, payload}, ...]}`   |
| `ApprovalRequest`   | `POST /api/v1/runner/runs/<run_id>/approvals/`           | Same shape as today's `_persist_approval`        |
| `RunAwaitingReauth` | `POST /api/v1/runner/runs/<run_id>/awaiting-reauth/`     |                                                  |
| `RunCompleted`      | `POST /api/v1/runner/runs/<run_id>/complete/`            |                                                  |
| `RunPaused`         | `POST /api/v1/runner/runs/<run_id>/pause/`               |                                                  |
| `RunFailed`         | `POST /api/v1/runner/runs/<run_id>/fail/`                |                                                  |
| `RunCancelled`      | `POST /api/v1/runner/runs/<run_id>/cancelled/`           |                                                  |
| `RunResumed`        | `POST /api/v1/runner/runs/<run_id>/resumed/`             |                                                  |
| `Bye`               | `DELETE /api/v1/runner/runners/<rid>/sessions/<sid>/`    | Or stop polling; server reaps after 2× interval. |

Every POST carries `Idempotency-Key: <message_id>` and is idempotent
on `(run_id, message_id)`.

Authorization rule for every `/runs/<run_id>/...` endpoint:

- Resolve `run = AgentRun.objects.select_related("runner").get(id=run_id)`.
- Require `run.runner_id == request.auth_runner.id`.
- If false, reject with 403 `run_not_owned_by_runner`.
- Implemented as a shared transport-service helper, not per-endpoint.

### 7.6 Session fencing (no two sessions for one runner)

Decision #16. Each runner has at most one active session.

- `POST /sessions/` for runner R evicts the prior session for R only.
  Sibling runners on the same machine are unaffected.
- **Eviction signaling**: Redis pub/sub on `session_eviction:<rid>`
  with body `{old_sid, new_sid}`. Each in-flight poll task is
  structured as `tokio::select!` (Python `asyncio.wait`) over:
  `XREADGROUP BLOCK 25000`, the eviction subscription, server
  timeout. First wake wins:
  - eviction → `409 session_evicted` with `superseded_by=<new_sid>`
  - timeout → `messages: []` normally
- The pub/sub channel is best-effort; if a worker missed the signal,
  the next poll's session-id check catches it (the row's
  `revoked_at` is set).
- **PEL handoff is done at session-open**, via paginated
  `XAUTOCLAIM` within `runner_stream:{rid}` from `consumer-{old_sid}`
  to `consumer-{new_sid}`. The old consumer name is retained for
  `2 × access_token_ttl_secs` post-eviction so a just-restarted
  daemon can still be fenced cleanly; after that window,
  `sweep_old_streams` reaps it.
- `session_id` is a required URL **path** segment on every poll/ack
  call; mismatched session_id → `409 session_evicted`.
- At most **one in-flight poll request per session**. A second
  concurrent poll on the same `session_id` returns 409
  `concurrent_poll`; the daemon treats this as a programmer error.

### 7.7 Liveness, summarized

Per decision #7:

- **Per-runner liveness via poll body's `status`**: each poll updates
  `Runner.last_heartbeat_at` and runs `_reap_stale_busy_runs`. A
  runner that stops polling for >`runner_offline_threshold_secs`
  (50s) is flipped `OFFLINE` by the `sweep_stale_runners` sweeper.
  Connection-level liveness no longer exists as a concept — there
  is no connection.
- **Session liveness via `RunnerSession.last_seen_at`**: updated on
  every poll. Sessions idle for >`2 × long_poll_interval_secs`
  (~50s) are revoked with reason `idle_timeout` by the
  `sweep_idle_sessions` sweeper.

### 7.8 Server-driven force refresh

Decision #17. New per-runner ServerMsg: `force_refresh`.

```json
{
  "type": "force_refresh",
  "reason": "signing_key_rotation" | "admin_request" | "suspected_leak",
  "min_rtg": 12
}
```

Mechanic:

- Cloud-side: write/update a `RunnerForceRefresh(runner, min_rtg,
reason)` row, then `XADD` a `force_refresh` message into
  `runner_stream:{rid}` so the runner's next poll receives it.
- Daemon-side: receive via the runner's poll, ack immediately, call
  `POST /api/v1/runner/runners/<rid>/refresh/`. The refresh endpoint
  deletes the `RunnerForceRefresh` row on success.
- Bulk operations (e.g., signing-key rotation) iterate active
  runners and bulk-insert + bulk-XADD. Cheap.

This shortens the staleness window (§5.4) from "up to TTL" to "up
to one poll round-trip" when the cloud needs to invalidate
existing access tokens before their natural expiry.

### 7.9 WebSocket reservation (per-run, opt-in)

The WS protocol (`runner/src/cloud/ws.rs`, the Channels consumer)
stays mounted at the existing path but is **only entered via a
one-shot ticket**:

```
POST /api/v1/runner/runs/<run_id>/stream/upgrade/
Authorization: Bearer <access_token>
Body: { "stream": "log" | "events" }
→ { "ws_url": "wss://.../stream/<ticket>", "ticket_expires_at": "..." }
```

The ticket is a 60-second random token bound to `(run_id, stream,
runner_id)` (the `runner_id` is resolved server-side from `run_id`
at upgrade-mint time). The WS handshake on the cloud accepts the
ticket exactly once and rejects anything else. The socket is
**per-run, time-bounded** (closes when the run ends), and has no
business authenticating against runner identity directly — the
ticket already encodes the authorization.

**Storage**: Redis key `ws_upgrade_ticket:{ticket_uuid}` →
`{run_id, stream, runner_id, expires_at}`, set with `EX 60`.
Consumed atomically via `GETDEL`. Reuse → reject.

v1 ships the endpoint and ticket store but no live consumer
(deferred to first real use case per §14). The door stays open
for live log tail and future media streams.

### 7.10 Sweepers and protocol-version rejection

**Periodic sweepers** (Celery beat or Django management command):

- `sweep_idle_sessions` (every 30s):
  `RunnerSession.objects.filter(revoked_at__isnull=True, last_seen_at__lt=now - 2*long_poll_interval_secs)`
  → set `revoked_at=now, revoked_reason='idle_timeout'`. Publish
  `session_eviction:<rid>` for each so straggler poll tasks detect.
- `sweep_stale_runners` (every 30s):
  `Runner.objects.filter(status=ONLINE, last_heartbeat_at__lt=now - runner_offline_threshold_secs)`
  → set `status=OFFLINE`. Does **not** revoke; just reflects
  observation. Re-attach revives.
- `sweep_old_streams` (every 5 min): three jobs.
  1. **Old-consumer reaping.** For each revoked session older than
     `2 × access_token_ttl_secs`, walk its consumer name
     (`consumer-{sid}`) and either `XAUTOCLAIM` any still-pending
     entries to the successor consumer if one exists, or
     `XGROUP DELCONSUMER runner_stream:{rid} runner-group:{rid} consumer-{sid}`
     to release them. Usually the successor claimed them at
     `POST /sessions/` time already.
  2. **PEL-aware trim of `runner_stream:{rid}`.** Per stream:
     ```
     summary = XPENDING runner_stream:{rid} runner-group:{rid}
     min_pending_id = summary.min_id  # may be None if PEL empty
     time_cutoff_id = ms_to_stream_id(now - runner_stream_min_retention_secs * 1000)
     if min_pending_id is None:
         safe_cutoff = time_cutoff_id
     else:
         safe_cutoff = min(time_cutoff_id, min_pending_id - 1)
     XTRIM runner_stream:{rid} MINID <safe_cutoff>
     ```
     Guarantees no PEL entry is trimmed.
  3. **Orphaned-stream deletion.** Delete `runner_stream:{rid}`
     whose runner is revoked or idle with `XLEN == 0` for >24h.
     Delete `runner_offline_stream:{rid}` idle >24h with
     `XLEN == 0`.
- `sweep_run_message_dedupe` (daily): delete `RunMessageDedupe`
  rows older than `run_message_dedupe_ttl_secs` (7 days).

**v3 protocol rejection** (decision #14). Two surfaces:

- **HTTP path**. `POST /sessions/` inspects an
  `X-Runner-Protocol-Version` header. Missing or `< 4` → `426
Upgrade Required`, body `{"error":
"protocol_version_unsupported", "minimum": 4, "upgrade_url":
"..."}`. No other endpoint enforces this (only reachable
  post-session-open).
- **WS path** (kept for the per-run upgrade ticket only, §7.9). The
  upgrade handler rejects daemons advertising `X-Runner-Protocol < 4`
  by sending close code 1008 with reason
  `protocol_version_unsupported`.

## 8. Ordering, idempotency, dedupe

- **Cloud → daemon**: monotonic Redis Streams ids within
  `runner_stream:{rid}`. Per-runner handler ordering is preserved
  inside the daemon by routing each poll's response directly to
  that runner's `RunnerLoop` mailbox. **Cross-runner ordering is
  not guaranteed** (and the new architecture makes this explicit:
  different runners are entirely independent at the transport
  layer).
  Delivery is **at-least-once**: ack happens only after the daemon's
  per-runner handler completes (decision #21, ack-on-handle); any
  in-flight crash or 5xx on the next poll causes redelivery.
  Per-instance inbound `mid` LRU (cap ~256, TTL ~5 min) drops a
  redelivered message that has already been processed.
- **Daemon → cloud**: each POST carries `Idempotency-Key` set to the
  runner-side `message_id`. Endpoint dedupes on `(run_id, message_id)`
  via `RunMessageDedupe` (decision #19). Insert-first-wins under a
  unique constraint; duplicate insert returns the stored success
  shape. Rows older than 7 days are periodically deleted.
- **Cancellation race**: when a `cancel` is queued and the run
  finishes naturally before the daemon polls, the cancel is dropped
  on next poll because the run is terminal — same as today's
  `consumers._finalize_run` logic, ports unchanged.

## 9. Timing & tunables

| Tunable                            | Default |
| ---------------------------------- | ------- |
| `long_poll_interval_secs`          | 25      |
| `access_token_ttl_secs`            | 3600    |
| `event_batch_max_age_ms`           | 250     |
| `event_batch_max_bytes`            | 65536   |
| `runner_offline_threshold_secs`    | 50      |
| `offline_stream_ttl_secs`          | 86400   |
| `offline_stream_maxlen`            | 1000    |
| `runner_stream_min_retention_secs` | 3600    |
| `run_message_dedupe_ttl_secs`      | 604800  |

All exposed in Django settings (`apple_pi_dash/settings/common.py`)
so production can tune without code changes.

## 9.1 Throttling

These endpoints are not user-interactive APIs and should not inherit
default user throttles.

- `POST /runners/<rid>/sessions/<sid>/poll`: no coarse DRF rate
  throttle. Safety enforced by protocol constraints:
  - one active session per runner
  - one in-flight poll per session
  - 25s server timeout
  - optional abuse backstop: reject sustained poll loops faster
    than 1 request / 5s for >3 consecutive requests with 429
    `poll_rate_exceeded`
- Upstream lifecycle/event POSTs: `RunnerRateThrottle`, keyed by
  `runner_id`, default budget sized for event batching: 600
  requests/minute burst, 300 sustained.
- Enrollment / refresh endpoints keep tighter auth-sensitive
  throttles keyed by remote IP.

## 10. Failure modes

| Symptom                                                   | Cause                                                    | Recovery                                                                                                                                                                                                                                                                                                        |
| --------------------------------------------------------- | -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Daemon gets 401 `access_token_expired` for one runner     | TTL elapsed                                              | That runner's client refreshes silently and retries the request once. Sibling runners unaffected.                                                                                                                                                                                                               |
| Daemon gets 401 `membership_revoked` for one runner       | Minting user lost workspace access                       | That runner shuts down (its `Runner` was just revoked server-side). Surface the reason in TUI/logs. Sibling runners owned by the same user fail their next refresh independently.                                                                                                                               |
| Daemon gets 401 `refresh_token_replayed` for one runner   | Old refresh token re-used after rotation                 | That runner shuts down. Operator re-enrolls if legitimate. The Runner row is already revoked.                                                                                                                                                                                                                   |
| Long-poll returns network error                           | Transient                                                | That runner's loop retries with exponential backoff capped at 30s. Other runners unaffected.                                                                                                                                                                                                                    |
| Cloud has a queued `cancel` but the run already completed | Race                                                     | Cancel dropped on next poll (run terminal). No-op.                                                                                                                                                                                                                                                              |
| Cloud restart / ASGI worker recycle                       | Routine                                                  | Outbox is in Redis. Next poll lands on a different worker and works unchanged. No thundering herd because polls are distributed across the 25s window.                                                                                                                                                          |
| Daemon gets 409 `session_evicted` for one runner          | Another daemon opened a new session for the same runner  | That runner shuts down its loop; the displacing daemon owns delivery. Operator-visible event in TUI/logs. Sibling runners unaffected.                                                                                                                                                                           |
| Daemon receives `force_refresh` for one runner            | Cloud invalidating that runner's access token before TTL | That runner refreshes inline before the next poll, then resumes.                                                                                                                                                                                                                                                |
| Daemon crashes mid-handle of a poll msg                   | Process killed before handler completes                  | Message stays in `consumer-{sid}`'s PEL. On daemon restart + new session for that runner, session-open `XAUTOCLAIM`s the PEL onto `consumer-{new_sid}`. First poll under the new session uses `XREADGROUP ... 0` to re-fetch; per-instance inbound `mid` LRU dedupes if the prior daemon had partially handled. |

## 11. Phased rollout

Each phase ships independently and leaves the system in a working
state.

### Phase 1 — Cloud: per-runner refresh + access token + MachineToken

- Schema migration: drop `Connection`, add the trust columns to
  `Runner`, rekey `RunnerSession` and `RunnerForceRefresh` to
  runner-scoped, add `MachineToken`.
- New endpoint: `POST /api/v1/runner/runners/<rid>/refresh/` — §5.3
  including live `is_workspace_member` check and lazy
  revoke-on-deny.
- New `RunnerAccessTokenAuthentication` DRF class — §5.4.
- New `MachineTokenAuthentication` DRF class — §5.6 (with
  per-request `is_workspace_member` check).
- Enrollment endpoint moved to `POST /api/v1/runner/runners/enroll/`
  and upgraded to mint refresh + access tokens (§5.1) plus
  conditional MachineToken bootstrap.
- `pidash auth login` flow on the cloud side — web UI mints a
  one-time ticket; the CLI redeems it for a MachineToken.

**Done when**: a runner can call refresh and get a fresh access
token; access-token verification works on a sample endpoint;
revoking workspace membership and refreshing yields 401
`membership_revoked` with `Runner.revoked_at` set; a new machine's
first runner enrollment also returns a MachineToken, and a second
runner enrollment from the same machine does not.

### Phase 2 — Cloud: per-runner sessions + streams + long-poll

- New `RunnerSession` model + migration (rekeyed by runner).
- New endpoints:
  - `POST /api/v1/runner/runners/<rid>/sessions/` — opens a
    per-runner session, evicts prior, ensures `runner_stream:{rid}`
    - `runner-group:{rid}`, paginated `XAUTOCLAIM` of prior
      consumer's PEL onto new consumer, runs `_apply_hello` flow,
      drains queued runs, returns Welcome (+ optional ResumeAck).
  - `DELETE /api/v1/runner/runners/<rid>/sessions/<sid>/` — clean
    shutdown.
  - `POST /api/v1/runner/runners/<rid>/sessions/<sid>/poll`
    (POST, not GET — body carries `ack` + `status`).
- Redis Streams outbox helpers:
  - `enqueue_for_runner(runner_id, msg)` — XADD into
    `runner_stream:{rid}` (no MAXLEN); offline policy if not.
  - `read_for_session(rid, sid, timeout_ms)` — `XREADGROUP GROUP
runner-group:{rid} consumer-{sid} ... BLOCK timeout_ms STREAMS
runner_stream:{rid} (0|>)`.
  - `ack_for_session(rid, [stream_id, ...])` —
    `XACK runner_stream:{rid} runner-group:{rid} <id1> [<id2> ...]`.
- Migrate `send_to_runner` to **dual-write**: existing Channels
  group + Redis stream for the runner's active session (if any).

**Done when**: a test client can open a session for a runner, poll,
receive a queued message, ack it via the next poll, and observe
that subsequent polls don't return it again. Concurrent
session-open evicts the prior session and returns
`409 session_evicted` to the displaced poll. Sibling runners on
the same machine unaffected by single-runner eviction.

### Phase 3 — Cloud: HTTP endpoints for runner-upstream events

- Implement every POST in §7.5, backed by the same handler
  functions today's WS consumer dispatches to. Extract handler
  bodies from `RunnerConsumer.on_run_started`, `on_run_event`,
  `on_approval_request`, etc. into transport-agnostic services
  (shared between WS path during phases 1–4 and new HTTP
  endpoints).
- Add `RunMessageDedupe` model + helper service. Idempotency on
  `(run, message_id)` mandatory, DB-backed.
- Add per-runner authorization guard for `/runs/<run_id>/...`
  endpoints: `run.runner_id == request.auth_runner.id` else 403
  `run_not_owned_by_runner`.
- Add `RunnerRateThrottle` keyed by `runner_id`.
- WS-upgrade ticket plumbing (§7.9): mint endpoint + Redis ticket
  store + `GETDEL` consume on handshake.

**Done when**: the entire WS-driven test suite has a parallel
HTTP-driven test suite that exercises every lifecycle transition
with identical expectations.

### Phase 4 — Daemon: switch to per-runner HttpLoops

- `runner/src/cloud/ws.rs`: kept; `ConnectionLoop` no longer dialed.
- New module `runner/src/cloud/http.rs` implementing:
  - `SharedHttpTransport` — clone-shared `reqwest::Client`,
    HTTP/2 keep-alive enabled, daemon-shared.
  - `RunnerCloudClient` per `RunnerInstance` — owns that runner's
    refresh token + access token + session_state, provides
    `refresh()`, `open_session()`, `close_session()`, `poll()`,
    `post_run_event()`, `post_run_lifecycle()`,
    `post_approval_request()`, `force_refresh_inline()`. Uses
    `SharedHttpTransport.http`.
  - `HttpLoop` per `RunnerInstance` — owns the long-poll loop
    targeting `runner_stream:{rid}` via that runner's session.
  - Per-runner `refresh_loop` task — fires ~5 min before access
    token expiry.
- `RunnerOut::send` retargets to the runner's `RunnerCloudClient`
  (per-runner dispatch; no shared `out_tx` mpsc).
- **No Demux**, **no `attach_emitter`**, **no shared mailbox map** —
  each `RunnerInstance` owns its full transport stack.
- Bump `WIRE_VERSION` / `protocol_version` to **4**. Cloud rejects
  3 with `426 upgrade required`.
- TUI surfaces "polling" / "refreshing" / "session_evicted" per
  runner.

**Done when**: `cargo test --workspace` passes including new
integration tests that drive each runner end-to-end against a
fake HTTP cloud; an end-to-end run completes against the real
cloud over HTTP for two concurrent runners on one daemon.

### Phase 5 — Cloud: retire WS as control plane

- `send_to_runner` stops dual-writing — pushes only to the runner's
  Redis stream.
- The Channels consumer's hot path (`receive_json` for control
  messages) is removed. The consumer is kept for the per-run WS
  upgrade path described in §7.9, with its handshake gated on the
  upgrade ticket.
- `_apply_hello`, `_resolve_connection_runner` (renamed
  `_resolve_runner`), group-add, online/offline transitions move
  from `consumers.py` into a shared service module callable from
  both the WS consumer (still used by per-run upgrade ticket) and
  the new session-open endpoint.

**Done when**: monitoring shows zero control traffic on the WS
endpoint over a 24h window; the WS endpoint logs nothing but
upgrade-ticket handshakes.

## 12. Test plan

- **Unit**: refresh-endpoint state machine per-runner (revoked,
  replayed, membership-lost, happy path); access-token verification
  (expired, bad signature, mismatched `rtg`, `min_rtg` rejection);
  outbox enqueue/drain/ack semantics on `runner_stream:{rid}`;
  idempotency dedupe.
- **Integration**: end-to-end Assign → Accept → RunEvent×N →
  RunCompleted over HTTP for one runner; same for two concurrent
  runners on one daemon (verifying isolation: a 5xx on runner A's
  poll doesn't disturb runner B); cancellation while polling;
  reconnection after server restart; refresh-then-resume after
  sleep > TTL.
- **Property/contract**: protocol-version 4 mismatch returns 426;
  v3 daemon rejected with the upgrade-required marker.
- **Security**: refresh-time workspace-membership recheck — kick a
  member, refresh fails, `Runner.revoke()` cascades, in-flight
  `AgentRun` cancelled, pinned QUEUED runs lose pin, pods
  re-drained. Up-to-TTL staleness asserted (a previously-issued
  access token continues to work for ≤TTL, not indefinitely).
- **Session fencing per-runner**: open a session for runner R, then
  open a second session for the same R from another process. The
  first session's in-flight poll returns `409 session_evicted`;
  the second session's first poll inherits the prior PEL via
  paginated `XAUTOCLAIM`. Sibling runner R' on the same machine
  unaffected. Variant: prior PEL > 1000 entries — verify full
  drain.
- **Force refresh**: queue a `RunnerForceRefresh` row + XADD
  `force_refresh`; observe the runner refresh inline before its
  next normal-cycle refresh.
- **MachineToken**: first enrollment per machine returns
  MachineToken; second runner enrollment from same machine does
  not; `pidash auth login` mints a token via web-UI ticket;
  workspace-membership lapse → next CLI call gets 401.
- **Per-runner liveness**: simulate one runner going silent (no
  polls for >50s); confirm only that runner flips OFFLINE; siblings
  continue working; stale busy-run reaping fires.

## 13. Open questions

- **`pidash connect --count N`** CLI sugar to enroll multiple runners
  in one operator command. Out of scope for this design; future
  CLI work.
- **Access-token signing**: HS256 is locked for v1. Revisit Ed25519
  only if a non-Django verifier becomes a real requirement.
- **Replay window for refresh-token rotation**: currently 1
  generation. If clock skew or network races cause spurious leak
  detections, widen to 2. Measure first.
- **Per-run WS upgrade ticket lifetime**: 60s in §7.9 is a guess.
  Tighten or loosen after first real consumer ships.
- **MachineToken expiration**: PAT-style with no TTL in v1.
  Consider rotating PATs in a future revision if compliance
  requires.
- **Mailbox / channel capacity tuning** under load — measure during
  Phase 4 integration testing (`daemon_module.md` §14).

## 14. Out of scope for v1

- Live log streaming via the per-run WS upgrade path (the canonical
  use case for §7.9). Designed for, not built in v1.
- Multi-region cloud (single-Redis outbox; cross-region replication
  left for later).
- SSE / WebTransport push transports.
- Bulk runner enrollment endpoint (`POST /runners/enroll/?count=N`).
  Designed-for at the protocol level (each runner is independent),
  not built in v1.
