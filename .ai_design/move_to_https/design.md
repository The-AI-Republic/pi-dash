# Cloud ↔ Daemon Transport: HTTPS Long-Poll + Refreshable Tokens

> Directory: `.ai_design/move_to_https/`
>
> Successor to the always-on WebSocket transport currently in
> `runner/src/cloud/ws.rs` and `apps/api/pi_dash/runner/consumers.py`.
>
> Two patterns are borrowed, from two different sources:
>
> 1. **Transport shape — borrowed from GitHub Actions self-hosted
>    runners.** Outbound HTTPS long-poll, explicit server-side session,
>    server-owned message ownership with explicit ack/delete (we use
>    Redis Streams; GH uses `ack`/`delete` REST endpoints). Sessions
>    are central to fencing and to message-ownership.
> 2. **Authentication shape — standard OAuth2 hygiene, not GH-specific.**
>    Short-TTL access token + long-lived refresh credential, refresh-token
>    rotation with replay detection, refresh as the chokepoint that
>    re-evaluates workspace membership. GitHub's runner uses tenant-scoped
>    `VssOAuthCredential` material with per-fetch authz on the service
>    side; we adopt a different shape that fits this codebase's session
>    model. The design's security rationale stands on OAuth2 norms, not
>    on GH precedent.
>
> The existing WebSocket protocol (`runner/src/cloud/protocol.rs`,
> Channels consumer) is **kept** as a future channel for data-heavy
> per-run streams (live log tail, large tool output, future media). It
> stops being the always-on connection.

## 1. Goal

- Eliminate the always-on stateful authenticated WebSocket as the
  control plane between the cloud and the daemon.
- Replace it with HTTPS long-poll endpoints for control traffic
  (assignments, cancels, approval decisions, config push, removal,
  lifecycle events) and ordinary POSTs for runner→cloud upstream
  events.
- Replace the long-lived `connection_secret` with a short-TTL access
  token + a long-lived refresh credential. The refresh endpoint is
  the **chokepoint** that re-evaluates whether the token's user is
  still a member of the connection's workspace, **at refresh time**
  (≤ access-token TTL of staleness for non-refresh requests; see §5.4).
- Preserve the multi-runner-per-machine architecture from
  `.ai_design/n_runners_in_same_machine/`: one connection-level
  session, with explicit per-runner attach/detach and per-runner
  liveness inside that session.
- Preserve the existing WS protocol/code so future data-heavy
  per-run streams can opt into a one-shot WS upgrade without
  re-introducing always-on stateful auth.

Non-goals:

- Renaming the auto-issued `APIToken` minted at connection
  enrollment for the `pidash` CLI (`apps/api/pi_dash/runner/views/connections.py:275-285`).
  That serves a different threat model (interactive user CLI traffic)
  and stays a long-lived PAT-style credential, separately revocable.
- Changing the runner ↔ codex/claude-code subprocess protocol. Only
  the runner ↔ cloud edge moves.

## 1.1 Architectural layering

This migration is a transport replacement, not a redesign. The
runner ↔ cloud edge has three planes; only one changes:

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
   `tasks.md` §3.3 + §5.2 for the cloud-side service extraction that
   makes the same handlers callable from both transports.
3. **Transport** — `runner/src/cloud/ws.rs` and the Channels consumer
   today; `runner/src/cloud/http.rs` and a set of DRF endpoints after
   Phase 4. **This plane is what gets replaced.** The WS code stays
   in the build for future opt-in per-run upgrade streams
   (decision #2, §7.9).

Four current variants — `Hello`, `Heartbeat`, `Bye`, `Ping` — are
absorbed into transport primitives rather than preserved as messages.
Their data still flows (POST attach body, poll-request `status[]`
vector, `DELETE` session, long-poll's own server timeout) but they
are no longer `ClientMsg`/`ServerMsg` envelopes after Phase 4. One
new variant — `force_refresh` (decision #17) — is added at the
schema layer to support the new auth model; it rides the same poll
path as every other `ServerMsg`.

This layering is what makes the migration cheap: every business-logic
call site (run lifecycle, approvals, events) recompiles unchanged;
only the dispatcher behind `RunnerOut` and the cloud-side handler
plumbing change. It is also what keeps the door open to bringing the
WS transport back later for a single per-run stream — the schema
plane already supports it.

## 2. Why now

The current design has three structural problems, two of them already
identified in the codebase, the third surfaced during review of
`runner/src/cloud/ws.rs` and `consumers.py`:

1. **Long-lived bearer = no live authorization.** The
   `connection_secret` is bound at mint time to a `Connection` row
   with `created_by` (user) and `workspace`. After mint, no further
   re-check of `is_workspace_member(created_by, workspace)` happens.
   If the minting user is removed from the workspace, their daemon
   keeps working until somebody explicitly revokes the connection.
   Relying on a `post_delete` signal on `WorkspaceMember` is a
   mitigation, not a boundary — anything that prevents the signal
   from firing (test fixture, dropped Celery job, race during
   member removal, refactor) re-opens the gap. This is fail-open by
   default.
2. **WS upgrade = one-time auth for a multi-hour session.** Even if
   per-request HTTP auth were live, the consumer authenticates once
   at WebSocket upgrade (`apps/api/pi_dash/runner/consumers.py:552-589`)
   and never re-validates. A nine-hour socket is a nine-hour blind
   spot.
3. **Stateful socket forces sticky load balancing.** Channels routes
   inbound frames to the consumer instance that holds the socket.
   Outbound `send_to_runner` (`apps/api/pi_dash/runner/services/pubsub.py`)
   already goes through a Channels group, so this is partially
   solved, but the cloud still has one open connection per runner
   that holds asgi-worker resources for the runner's lifetime,
   complicating horizontal scaling and rolling deploys (every
   restart drops every socket; daemons reconnect in a thundering
   herd).

The control plane is **not** a real-time streaming workload. Approvals
are human-in-the-loop and tolerate seconds of latency. Heartbeats
already run at 25-second intervals. Runner→cloud lifecycle events are
discrete and infrequent. The only event flow that is plausibly
latency-sensitive is `RunEvent` (per-tool-call/per-token output) — and
that's exactly the kind of "data-heavy" stream we want to keep the WS
protocol around to support, on demand, per run.

## 3. Decisions locked in

| #   | Question                                                       | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| --- | -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Replace WS as the always-on control plane?                     | Yes. Control traffic moves to HTTPS long-polling + per-request POSTs.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| 2   | Keep the WS protocol code?                                     | Yes. Reserved for **per-run, opt-in, time-bounded** data-heavy streams (live log tail, future media). No always-on socket. Authentication for a WS upgrade is a one-shot ticket minted by the access-token-bearing daemon (§7.9).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| 3   | Token shape                                                    | Two tokens. **Refresh token** (long-lived, hashed in DB, on-disk 0600 in the daemon's existing credentials file). **Access token** (~1h TTL, self-contained signed token, daemon holds in memory only). Replaces the current single `connection_secret`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| 4   | Where does workspace-membership authorization happen?          | At the refresh endpoint, on every refresh. Access-token verification is signature + `exp` (no DB hit) plus a single indexed point query on `Connection` to read `refresh_token_generation` for the `rtg` lower-bound check. No live workspace-membership check on the request hot path. Authorization staleness is therefore bounded by access-token TTL (≤1h by default). Sensitive endpoints may opt into per-request live re-check (§5.4).                                                                                                                                                                                                                                                                                                 |
| 5   | What happens when refresh is denied because membership lapsed? | **Lazy revoke-on-deny.** The refresh endpoint calls `Connection.revoke()` on the failing path. That cascades to runners, cancels in-flight `AgentRun`s, drops pinned-runner pins, and re-drains pods so other runners can pick up the orphaned work. No separate sweeper job needed; eventual consistency is bounded by the access-token TTL.                                                                                                                                                                                                                                                                                                                                                                                                 |
| 6   | Refresh-token rotation                                         | Rotate on every successful refresh. Store **the previous token's hash** in `Connection.previous_refresh_token_hash` (single-slot history; cleared on successful rotation of the new token). Lookup at refresh time tries `refresh_token_hash` first; if that misses but `previous_refresh_token_hash` matches, treat as a leak (the daemon presented a token that was already rotated away from): revoke the connection. Matches OAuth2 best practice for installed clients without changing token shape.                                                                                                                                                                                                                                     |
| 7   | Heartbeat / per-runner liveness                                | The poll **request body** carries a per-runner status vector `[{runner_id, status, in_flight_run, ts}, ...]`. The server applies each entry: updates `Runner.last_heartbeat_at`, runs the existing `_reap_stale_busy_runs` logic per entry. Connection-level liveness alone is insufficient — one surviving runner cannot vouch for dead siblings. The dedicated `Heartbeat` ClientMsg goes away; its fields move into the poll body.                                                                                                                                                                                                                                                                                                         |
| 8   | Outbox backing store                                           | **Redis Streams**, **one persistent stream per connection** (`connection_stream:{cid}`), **one persistent consumer group per connection** (`connection-group:{cid}`), and **one consumer name per session** (`consumer-{sid}`). Every control-plane message in the stream carries `type`, `mid`, and optional `runner_id`. `XREADGROUP` against `consumer-{sid}` does **not consume** — entries remain in that consumer's PEL until `XACK`. On session eviction, the replacement session `XAUTOCLAIM`s the prior consumer's pending entries (paginated, full drain) **within the same stream and group**. Trimming is sweeper-driven and PEL-aware (no inline `MAXLEN`), so unacked entries are never evicted under at-least-once. See §7.4.  |
| 9   | Ordering, dedupe, and ack model                                | Stream IDs are monotonic within `connection_stream:{cid}`. Ack body is the **explicit flat list** `["<stream_id_1>", "<stream_id_2>", ...]` containing every stream id from the previous poll response that the daemon has finished handling (per decision #21). Server issues `XACK connection_stream:{cid} connection-group:{cid} <id1> [<id2> ...]` (XACK takes exact IDs, not a range). The `Envelope.message_id` (mid) stays as an application-level dedupe key so redelivery from PEL after a daemon crash is not processed twice.                                                                                                                                                                                                      |
| 10  | Connection-level session, per-runner attach                    | Long-poll runs at the **session** level (one session per connection). The session is created with `POST /sessions/`; runners then attach with per-runner `POST /sessions/<sid>/runners/<rid>/attach/`, which preserves today's per-runner Hello semantics: populates `authorised_runners`, marks the runner online, applies metadata, may resume in-flight. Detach is symmetric. See §7.1 and `consumers.py:336-363` for current behavior being preserved.                                                                                                                                                                                                                                                                                    |
| 11  | Channel for `RunEvent`                                         | Batched POST `/api/v1/runner/runs/<run_id>/events/`, body `{"events": [...]}`. Daemon batches by time (≤ 250ms) **or** size (≤ 64 KB), whichever fires first. Phase 5 may upgrade per-run to a WS stream for runs flagged data-heavy.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| 12  | Auto-issued `APIToken` at enrollment                           | Unchanged. Different threat model (interactive user CLI), independent revocation. Documented at §1 non-goals.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| 13  | Pre-existing daemons                                           | None in production. The protocol described here is the only one shipped. The cloud serves the new endpoints from day one; the WS endpoint stays mounted but is no longer dialed by the daemon's main loop. Step-down of the WS endpoint from the control plane happens with the protocol-version bump in Phase 4.                                                                                                                                                                                                                                                                                                                                                                                                                             |
| 14  | Protocol version                                               | Bump the cloud-acknowledged `protocol_version` (currently 3 in `apps/api/pi_dash/runner/views/register.py:20` and `runner/src/cloud/protocol.rs`) to **4**. The bump signals "control plane is HTTP; WS is opt-in per-run." Older daemons advertising version 3 are rejected with a clear error pointing at the upgrade path.                                                                                                                                                                                                                                                                                                                                                                                                                 |
| 15  | TTLs                                                           | Access token: **1 hour**. Refresh token: **no fixed expiry** (revocable; tied to the Connection row). Long-poll timeout: **25 seconds** server-side (matches existing `HEARTBEAT_INTERVAL_SECS`). Daemon recovers by re-polling immediately on empty response.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| 16  | Session fencing                                                | Each connection has at most **one active session**. `POST /sessions/` evicts any prior session for the same connection: the prior `session_id` is marked revoked, its in-flight long-poll returns `409 session_evicted`. PEL claim from the old session into the new one happens at **`POST /sessions/`** time (§7.1, §7.6), on the persistent `connection_stream:{cid}` within the persistent `connection-group:{cid}`, by reassigning entries from `consumer-{old_sid}` to `consumer-{new_sid}` via paginated `XAUTOCLAIM`. `session_id` is a required URL **path** segment on every poll/ack/attach/detach call; mismatched session_id is rejected with `409 session_evicted`. Prevents two daemons fighting over one connection's outbox. |
| 17  | Server-driven force refresh                                    | A `force_refresh` ServerMsg can be queued to make the daemon refresh its access token now (out-of-cycle). Use cases: signing-key rotation, suspected leak, admin-initiated re-authz before the natural TTL. Daemon treats it as a high-priority message: refreshes inline, then continues normal polling. Mirrors GitHub Actions' `ForceTokenRefresh`.                                                                                                                                                                                                                                                                                                                                                                                        |
| 18  | Offline enqueue policy                                         | If a runner has **no active session**, control messages are not queued indefinitely. `assign` is rejected at scheduling time unless the runner is attached; non-run-specific control messages (`config_push`, `remove_runner`, `revoke`) may queue in a bounded per-runner offline stream with a **24h TTL / 1000-entry cap**, whichever hits first.                                                                                                                                                                                                                                                                                                                                                                                          |
| 19  | Upstream idempotency store                                     | v1 uses a dedicated DB table `RunMessageDedupe(run_id, message_id, created_at)` with a unique constraint on `(run_id, message_id)`. No JSON-on-row LRU, no in-memory-only cache. A periodic cleanup job deletes rows older than 7 days.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| 20  | Rate limiting                                                  | Runner transport endpoints are authenticated machine-to-cloud traffic and use **connection/session scoped throttles**, not end-user throttles. Poll: effectively unthrottled within protocol bounds (one in-flight poll per session, max 1 request/5s burst tolerance). Upstream POSTs: token-bucket per connection with generous defaults sized for event batches.                                                                                                                                                                                                                                                                                                                                                                           |
| 21  | Delivery semantics — ack-on-handle, not ack-on-receive         | The daemon adds a stream id to the next poll's `ack` list **only after the per-runner handler has completed processing**, not when `HttpLoop` dispatches into the mailbox. Plumbing: per-runner ack-back channel from `RunnerLoop` to `HttpLoop`. Result: protocol is at-least-once. PEL on session restart re-delivers any handled-but-not-yet-acked or fetched-but-not-yet-handled entries; the daemon's per-instance inbound `mid` LRU dedupes the redelivery so the handler runs at most once.                                                                                                                                                                                                                                            |

## 4. Conceptual model

| Concept                | What it is                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Connection**         | Same as today: per-machine bond with `created_by` and `workspace`. The unit of trust the refresh token authenticates against.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| **Refresh token**      | Long-lived credential the daemon stores at 0600 alongside the existing `credentials.toml`. Single-use rotation: each refresh consumes it and returns the next. Authenticates **against the user's current state**, not a snapshot.                                                                                                                                                                                                                                                                                                                                                                             |
| **Access token**       | Short-TTL signed token (HS256 with a server-side secret, or Ed25519 — see §5.2). Self-contained: payload includes `connection_id`, `workspace_id`, `user_id`, `iat`, `exp`. Verified statelessly on every API request.                                                                                                                                                                                                                                                                                                                                                                                         |
| **Session**            | Server-side row that owns delivery for a connection. Created by `POST /sessions/`; one active session per connection (newer evicts older, §7.6). Identified by `session_id` carried on every poll and ack. The session is the consumer identity that owns the connection stream's PEL.                                                                                                                                                                                                                                                                                                                         |
| **Runner attach**      | Per-runner state inside a session. Created by `POST /sessions/<sid>/runners/<rid>/attach/` after the connection-level session exists. Replaces today's per-runner `Hello`: marks the runner online, populates server-side authorisation, applies metadata, may resume in-flight. Detach (or session eviction) cleans up.                                                                                                                                                                                                                                                                                       |
| **Long-poll**          | Daemon's only persistent activity. One open `POST /sessions/<sid>/poll` per session, ≤25s server timeout, returns 0..N pending control messages from the connection stream. Request body carries the per-runner status vector (heartbeat + in-flight) and a flat ack list (stream ids of messages the daemon has finished handling since the last poll).                                                                                                                                                                                                                                                       |
| **Outbox**             | Redis Streams, **one stream per connection** (`connection_stream:{cid}`, persistent), **one consumer group per connection** (`connection-group:{cid}`, persistent), **per-session consumer name** (`consumer-{sid}`). `send_to_runner` / `send_to_connection` → `XADD` (no inline `MAXLEN` — see §7.4 retention contract); long-poll → `XREADGROUP BLOCK` against `consumer-{sid}` (no-consume; PEL retains until XACK); ack → `XACK <stream> <group> <id1> [<id2> ...]`. On session evict, paginated `XAUTOCLAIM` reassigns `consumer-{old_sid}`'s PEL to `consumer-{new_sid}` — within stream, within group. |
| **WS (legacy/opt-in)** | Existing `runner/src/cloud/ws.rs` + Channels consumer. Reachable only via the per-run upgrade endpoint (§7.9). Not used by the daemon's connection loop after Phase 4.                                                                                                                                                                                                                                                                                                                                                                                                                                         |

## 5. Authentication

### 5.1 Token issuance

`POST /api/v1/runner/connections/enroll/` (existing endpoint at
`apps/api/pi_dash/runner/views/connections.py:210-298`) is extended:

- Continues to consume the one-time enrollment token.
- Returns **two** tokens instead of one `connection_secret`:
  - `refresh_token`: opaque random 32-byte base64. Hashed into
    `connection.refresh_token_hash` (renamed from `secret_hash`;
    see §6 for migration). The token itself carries no decoded
    fields; replay detection works via the previous-hash
    single-slot history (§5.3, decision #6).
  - `access_token`: signed token with `exp = iat + 3600`.
- `connection.refresh_token_generation` (new column, default 0) is
  incremented to 1.
- `connection.previous_refresh_token_hash` is `NULL` on first
  issuance.
- The auto-issued `APIToken` for the `pidash` CLI is unchanged.

Response shape:

```json
{
  "connection_id": "...",
  "refresh_token": "rt_...",
  "access_token": "at_...",
  "access_token_expires_at": "...",
  "refresh_token_generation": 1,
  "name": "...",
  "workspace_slug": "...",
  "long_poll_interval_secs": 25,
  "protocol_version": 4,
  "api_token": "..."
}
```

### 5.2 Access-token format

Self-contained signed payload. No DB lookup on the hot path. Two
candidates:

- **HS256 JWT** signed with a server-side rotating key
  (`SECRET_KEY`-derived per environment). Trivial to verify in
  Django and the runner.
- **Ed25519** with a pinned public key in the daemon. Preferred if
  we want to allow third-party verifiers (e.g. proxy in front of
  the API). Adds a new dependency on the runner side.

**Decision**: HS256 for v1. Switch to asymmetric only if a use case
appears. Keep the verification logic isolated behind a single helper
so the algorithm change is one file.

**Key storage / rotation contract (locked for v1):**

- Signing keys live in Django settings as an **ordered key ring**,
  e.g. `RUNNER_ACCESS_TOKEN_KEYS = [{"kid": "2026-04-1", "secret":
"...", "status": "active"}, {"kid": "2026-02-1", "secret": "...",
"status": "verify_only"}]`.
- Exactly one key is `active` for minting. Any number may be
  `verify_only` during rotation overlap.
- The daemon does **not** need key material; it never verifies
  access tokens locally in v1. It treats them as opaque bearer
  tokens issued by the cloud.
- Rotation procedure:
  1. Add new key as `active`, demote old `active` to `verify_only`.
  2. Queue `force_refresh` to active sessions.
  3. After `access_token_ttl_secs + safety_margin`, remove the old
     key from the ring.
- `Connection.access_token_signing_key_version` is retained only as a
  future migration hook and is **not consulted in v1**. The JWT
  itself carries the authoritative key identifier.

Payload:

```json
{
  "kid": "2026-04-1",
  "iss": "pi-dash-cloud",
  "sub": "<connection_id>",
  "uid": "<user_id>",
  "wid": "<workspace_id>",
  "iat": 1714080000,
  "exp": 1714083600,
  "rtg": 1
}
```

`rtg` is the refresh-token generation at the moment this access token
was minted. The cloud rejects access tokens whose `rtg` is older than
`connection.refresh_token_generation - 1` (one-generation grace handles
in-flight requests during rotation).

Verification order:

1. Verify signature by `kid`; reject unknown `kid`.
2. Verify `exp`.
3. Verify `rtg` against the lower bound.
4. If a queued `force_refresh.min_rtg` exists for the connection,
   reject tokens with `rtg < min_rtg` **even if** they satisfy the
   normal one-generation grace. `min_rtg` is the stronger rule.

### 5.3 Refresh endpoint

`POST /api/v1/runner/connections/<connection_id>/refresh/`

Authentication: bearer the **refresh token** in the `Authorization`
header. No access token required (the daemon may have an expired
one).

Logic, in order, in a single transaction with `select_for_update` on
the Connection row:

1. Look up `Connection` by `id=connection_id`. If not found → 401
   `invalid_refresh_token`.
2. If `connection.revoked_at IS NOT NULL` → 401 `connection_revoked`.
3. **Token-hash match decision**:
   - If `hash(presented_token) == connection.refresh_token_hash` →
     happy path; proceed to step 4.
   - Else if `connection.previous_refresh_token_hash IS NOT NULL`
     and `hash(presented_token) == connection.previous_refresh_token_hash`
     → **leak detected**. Daemon presented a token that was already
     rotated away from. Call `connection.revoke()`. Return 401
     `refresh_token_replayed`.
   - Else → 401 `invalid_refresh_token`. (No replay claim; could be
     a stale, mistyped, or never-issued token.)
4. Live-check: `is_workspace_member(connection.created_by, connection.workspace_id)`.
   If false → **lazy revoke**. Call `connection.revoke()`. Return 401
   `membership_revoked`.
5. Mint a new refresh token. Atomically:
   - Move the current `refresh_token_hash` into
     `previous_refresh_token_hash`.
   - Set `refresh_token_hash` to the new token's hash.
   - Increment `refresh_token_generation`.
   - Mint a new access token with the new `rtg`.
6. Delete any `RunnerForceRefresh` row for this connection — the
   force-refresh has been honored by this rotation.
7. Return both tokens.

The daemon writes the new refresh token to disk **before** discarding
the old one in memory, so a crash between server step 7 and the
daemon's disk-write does not strand the daemon. If that crash window
is hit, the daemon comes back holding the _old_ token. On retry, the
server sees that as the `previous_refresh_token_hash` slot match and
revokes the connection (replay detection cannot distinguish "real
leak" from "client-side crash window"). Recovery is to re-enroll.
This is the same crash-window OAuth2 clients accept; the rate is low
enough to ignore for v1.

### 5.4 Authentication for every other endpoint (refresh-time authz, not live)

A single DRF authentication class — `AccessTokenAuthentication` —
replaces `ConnectionBearerAuthentication` for the new endpoints.
Behavior:

- Pull `Authorization: Bearer <jwt>` from the request.
- Verify signature and `exp`. On failure → 401 `access_token_expired`
  (so the daemon knows to refresh, not re-enroll).
- Verify `rtg` against `connection.refresh_token_generation - 1` lower
  bound. The connection lookup is a single indexed point query.
- Set `request.auth_connection` and `request.auth_runner` (when the
  URL kwargs name a runner).

**Naming this honestly**: this is **not** "live authorization" in the
sense GitHub uses (where every fetch can reject a deleted runner
immediately). This is **authorization re-evaluated on each refresh
cycle**, with up to access-token-TTL staleness on every other request.
The named tradeoff:

- A user removed from the workspace at T=0 can keep their daemon
  acting on previously-issued access tokens until at most T+TTL
  (default 1h). At T+TTL the next refresh fails, `Connection.revoke()`
  cascades, and the connection is dead.
- The cloud can shorten that window further by issuing a queued
  `force_refresh` (decision #17, §7.8), which makes the daemon
  refresh immediately on next poll round-trip — typically within
  seconds, not the full TTL.

If a specific endpoint cannot tolerate up-to-TTL staleness (e.g. a
write that exposes cross-workspace data, an admin-impersonation
trigger), that endpoint opts into a live workspace-membership
re-check explicitly. Default off.

### 5.5 What lives where on disk

The daemon's existing credentials file (`~/.config/apple-pi-dash-runner/credentials.toml`, 0600) gains a `[refresh]` block:

```toml
[connection]
id = "..."
name = "..."

[refresh]
token = "rt_..."
generation = 7
issued_at = "..."

[api_token]
token = "..."  # unchanged, used by `pidash` CLI
```

The access token never touches disk.

## 6. Data model

Additive changes to `apps/api/pi_dash/runner/models.py`:

- **`Connection`**:
  - Rename `secret_hash` → `refresh_token_hash` (string, indexed,
    holds the hash of the **current** refresh token).
  - Rename `secret_fingerprint` → `refresh_token_fingerprint`.
  - New `refresh_token_generation: PositiveIntegerField(default=0)`.
    Bumped on every successful refresh. Used as the lower bound for
    accepting access tokens.
  - New `previous_refresh_token_hash: CharField(max_length=128, blank=True, default="")`.
    Single-slot history: holds the hash of the token that was
    rotated away from on the most recent successful refresh.
    Cleared (set to "") only if the connection is revoked. Used by
    §5.3 step 3 to detect replay.
  - New `access_token_signing_key_version: PositiveIntegerField(default=1)`.
    Reserved for a future signing-key rotation. Not used in v1.

These are renames + adds, not destructive removals. Old code paths
that reference `secret_hash` move atomically to the new name in the
same migration.

- No changes to `Runner`, `Pod`, `AgentRun`.
- No changes to the auto-issued `APIToken` flow.

New table — **`RunnerSession`** (the unit of delivery ownership, §7.1, §7.6):

| Column             | Type                  | Notes                                                                   |
| ------------------ | --------------------- | ----------------------------------------------------------------------- |
| `id`               | UUIDField (PK)        | `session_id` carried on every poll/ack                                  |
| `connection`       | FK → Connection       | `on_delete=CASCADE`                                                     |
| `created_at`       | DateTimeField         | `auto_now_add`                                                          |
| `last_seen_at`     | DateTimeField         | Updated on every poll request                                           |
| `revoked_at`       | DateTimeField (null)  | Set on eviction (`POST /sessions/` from a new daemon) or session-DELETE |
| `revoked_reason`   | CharField (32, blank) | `superseded`, `client_close`, `idle_timeout`, `connection_revoked`      |
| `protocol_version` | PositiveIntegerField  | Version negotiated at session-open                                      |
| `host_label`       | CharField (255)       | Reported by daemon at session-open                                      |
| `agent_versions`   | JSONField             | Reported agent CLI versions at session-open                             |

Constraint: only one active session per connection (`UniqueConstraint`
on `connection` filtered by `revoked_at IS NULL`).

New table — **`RunMessageDedupe`** (decision #19):

| Column       | Type              | Notes                                     |
| ------------ | ----------------- | ----------------------------------------- |
| `id`         | BigAutoField (PK) |                                           |
| `run`        | FK → AgentRun     | `on_delete=CASCADE`                       |
| `message_id` | CharField(64)     | Runner-supplied `mid` / `Idempotency-Key` |
| `created_at` | DateTimeField     | `auto_now_add`                            |

Constraint: `UniqueConstraint(fields=["run", "message_id"], name="run_message_dedupe_unique")`.

New table — **`RunnerForceRefresh`** (ephemeral command state for decision #17):

| Column       | Type                  | Notes                                                     |
| ------------ | --------------------- | --------------------------------------------------------- |
| `connection` | OneToOne → Connection | one outstanding forced-refresh floor per connection       |
| `min_rtg`    | PositiveIntegerField  | optional stronger lower bound than normal grace           |
| `reason`     | CharField(32)         | `signing_key_rotation`, `admin_request`, `suspected_leak` |
| `created_at` | DateTimeField         |                                                           |

Per-runner attach state lives in-memory inside the asgi worker for
the life of the active poll; durable per-session-per-runner state
that needs to survive worker restarts (e.g. last-acked stream id) is
recoverable from Redis Streams' consumer group PEL, so no DB column
is required for it.

Migration sequence (single migration since there is no production
data per `decisions #13` and prior fresh-install simplification in
`.ai_design/issue_runner/design.md`):

1. Add `refresh_token_generation`, `previous_refresh_token_hash`,
   `access_token_signing_key_version`.
2. Rename `secret_hash` → `refresh_token_hash` and
   `secret_fingerprint` → `refresh_token_fingerprint`.
3. Existing `Connection` rows have `refresh_token_generation = 0`
   and `previous_refresh_token_hash = ""` and are forced through
   re-enrollment on first daemon start (the daemon detects a
   credentials file lacking `[refresh]` and prompts).
4. Add `RunnerSession`, `RunMessageDedupe`, and `RunnerForceRefresh`.

## 7. Wire protocol mapping

Every current ClientMsg / ServerMsg variant maps to an HTTP endpoint.
The WS protocol enums in `runner/src/cloud/protocol.rs` stay as the
**body schemas** so the runner code can largely re-use serialization.

The model is **session-bound, with per-runner attach inside the
session**. This preserves the multi-runner-per-machine semantics
already implemented in `consumers.py:336-363` (per-runner `Hello` →
populates `authorised_runners`, joins the runner's Channels group,
marks online, applies metadata, may resume in-flight). One session
per connection; multiple runners attach into it.

### 7.1 Session lifecycle (replaces connection-level `Hello`)

```
POST /api/v1/runner/connections/<cid>/sessions/
Authorization: Bearer <access_token>
Body: { "host_label": "...", "agent_versions": {...} }
→ 201 {
    "session_id": "...",
    "server_time": "...",
    "long_poll_interval_secs": 25,
    "protocol_version": 4
  }
```

Behavior on the server (mirrors today's WS upgrade behavior):

- Verify the access token; resolve `Connection`.
- **Evict any prior active session** for this connection: mark
  `revoked_at` on the old session row, signal any in-flight poll on
  it to return `409 session_evicted`. The persistent stream
  `connection_stream:{cid}` and consumer group
  `connection-group:{cid}` are **not** session-keyed and survive
  eviction. What changes is the **consumer name**: the new session's
  polls will read against `consumer-{new_sid}`. Decision #16.
- **Ensure the persistent connection stream and consumer group exist**
  (idempotent):

  ```
  XGROUP CREATE connection_stream:{cid} connection-group:{cid} $ MKSTREAM
  ```

  Ignore `BUSYGROUP` in steady state.

- **Claim any old session PEL into the new consumer name** with a
  paginated `XAUTOCLAIM` loop (Redis 6.2+). `XAUTOCLAIM` is the
  purpose-built paginated handoff primitive and avoids the
  `XPENDING ... COUNT N` truncation bug if the prior consumer's PEL
  exceeds a single page:

  ```
  cursor = "0-0"
  loop:
      reply = XAUTOCLAIM connection_stream:{cid} connection-group:{cid}
                         consumer-{new_sid}
                         min-idle-time=0
                         start=<cursor>
                         COUNT 1000
                         JUSTID
      cursor = reply.next_cursor      # "0-0" when fully drained
      if cursor == "0-0": break
  ```

  `XAUTOCLAIM` filters out any IDs whose payloads were trimmed away —
  those are returned as a "deleted entries" list in the reply and are
  removed from the PEL automatically, so the new session never sees
  tombstone IDs. We rely on §7.4's PEL-aware retention contract to
  make those tombstones a non-issue in steady state. This is the
  normal Redis handoff path: same stream, same group, different
  consumer.

- Create a new `RunnerSession` row.
- Return the synchronous `Welcome` payload.

```
DELETE /api/v1/runner/connections/<cid>/sessions/<sid>/
```

Clean shutdown; server reaps the session row and its consumer
ownership. The persistent connection stream/group survive. If the
daemon disappears without calling DELETE, the session is reaped
after `2 × long_poll_interval_secs` of no poll activity.

### 7.2 Per-runner attach (replaces per-runner `Hello`)

```
POST /api/v1/runner/connections/<cid>/sessions/<sid>/runners/<rid>/attach/
Authorization: Bearer <access_token>
Body: {
  "version": "...",
  "os": "...",
  "arch": "...",
  "status": "idle" | "busy",
  "in_flight_run": null | "<uuid>",
  "project_slug": "..."   // optional, current behavior
}
→ 200 {
    "welcome": { ... existing per-runner Welcome payload ... }
  }
```

Server behavior — exactly mirrors `consumers.py:336-363`:

1. Validate runner belongs to the connection (current `_resolve_connection_runner`).
2. Validate `project_slug` matches `runner.pod.project.identifier` if provided.
3. Insert into the session's `authorised_runners` set.
4. Run `_apply_hello` (metadata save + stale-busy reaping with
   `in_flight_run`).
5. Mark runner `ONLINE`.
6. Drain any queued runs (`drain_for_runner_by_id`).
7. If `in_flight_run` is set, kick off `_resume_run` and return a
   `ResumeAck` body alongside the Welcome.

**Per-runner offline handoff happens here**:

a. **Drain the offline buffer into the live connection stream**:

```
XRANGE runner_offline_stream:{rid} - +
→ for each entry: XADD connection_stream:{cid} fields:{runner_id: rid, ...msg, offline_id: <orig_id>}
→ XDEL runner_offline_stream:{rid} <orig_id>
```

Bounded by §7.4's offline cap (1000 entries per runner).
Atomic per entry; partial failure leaves the offline stream in
a recoverable state.

If the daemon's `attach/` call fails after this drain but before the
daemon begins polling, the copied entries are still safe: they now
sit in the connection stream and will be picked up by the same
session once it resumes polling, or by the successor session if this
session is evicted before consuming them.

Detach is symmetric:

```
DELETE /api/v1/runner/connections/<cid>/sessions/<sid>/runners/<rid>/
```

— marks the runner offline within this session. The runner row is
unchanged; another session/runner attach can revive it.

**Detach interaction with an in-flight poll.** When a runner detaches
mid-poll, the existing `XREADGROUP BLOCK` for that session may
already be waiting on the connection stream while the attached-runner
set changes. The detach request publishes a Redis pub/sub message on
`session_attach_change:<sid>` with `{runner_id, op: "detach"}`. The
poll task's `tokio::select!` (Python `asyncio.wait`) listens on this
channel and, on receipt, returns immediately with `messages: []`
and the daemon's next poll uses the new attached set. Cloud-side:
between detach completing and the next poll arriving, no new entries
are enqueued for that runner (decision #18 / §7.4 routes to the
offline stream). Any entries already in the consumer's PEL stay
there and are redelivered by the same consumer until acked; if the
session is replaced before they are acked, the successor session
claims them at `POST /sessions/` time (§7.1).

### 7.3 Long-poll (replaces cloud→daemon `ServerMsg` push)

```
POST /api/v1/runner/connections/<cid>/sessions/<sid>/poll?timeout=25
Authorization: Bearer <access_token>
Body — note this endpoint is `POST` rather than `GET`, because the
request carries the ack list and per-runner status. (Some HTTP
intermediaries strip GET bodies; POST avoids the ambiguity.)
{
  "ack": ["<stream_id_1>", "<stream_id_2>", "<stream_id_3>"],
  "status": [
    { "runner_id": "<runner_id_a>", "status": "idle",
      "in_flight_run": null, "ts": "..." },
    { "runner_id": "<runner_id_b>", "status": "busy",
      "in_flight_run": "<uuid>", "ts": "..." }
  ]
}
```

`ack` is the **explicit flat list** of every stream id from the
previous poll response that the daemon has now finished handling
(per decision #21, ack-on-handle). It is intentionally not keyed by
runner so both runner-scoped and connection-scoped frames use the
same acknowledgment path. Empty list is fine if the daemon
dispatched nothing in the previous response, or dispatched but
hasn't finished handling yet.

**`session_id` is mandatory.** Polls without it, or with a stale
session_id, are rejected with `409 session_evicted` and the daemon
shuts down its loop (it has been displaced; another daemon owns the
connection now — see decision #16).

Server side, in this order:

1. Verify session is active; reject with `409 session_evicted` if
   not.
2. Update `RunnerSession.last_seen_at`. (This is the
   connection-level liveness signal — proves the daemon loop is
   alive.)
3. For each `status[]` entry: update the runner's
   `last_heartbeat_at`, run `_reap_stale_busy_runs(runner, entry)`.
   This is the per-runner liveness signal; decision #7. **One poll
   = N runner heartbeats**, not one connection heartbeat.
   Validation rules:
   - `runner_id` must already be attached to this session, else 400
     `unknown_runner_in_status`.
   - Omitting an attached runner from `status[]` for one poll is
     tolerated; omitting it for `runner_offline_threshold_secs`
     makes it OFFLINE.
   - Empty `status[]` is accepted only when the session currently has
     zero attached runners; otherwise 400 `missing_runner_status`.
4. If `ack` is non-empty, issue
   `XACK connection_stream:{cid} connection-group:{cid} <id1> [<id2> ...]`.
   XACK takes the explicit ID list; entries are removed from this
   consumer's PEL.
5. Issue one `XREADGROUP` against the connection stream. The first
   poll after `POST /sessions/` uses `0` (re-fetch this consumer's
   PEL — newly-claimed entries from the prior session):

   ```
   XREADGROUP GROUP connection-group:{cid} consumer-{sid}
              COUNT 100 BLOCK 25000
              STREAMS connection_stream:{cid}
                      0
   ```

   Subsequent polls use `>` (only entries not yet delivered to any
   consumer in this group) once the PEL has drained:

   ```
   XREADGROUP GROUP connection-group:{cid} consumer-{sid}
              COUNT 100 BLOCK 25000
              STREAMS connection_stream:{cid}
                      >
   ```

   The poll-handler tracks per-session "PEL drained" state in Redis
   (`session_pel_drained:{sid}`), set after a `0`-based read returns
   empty, and uses that to choose `0` vs `>`.

6. On any read returning, filter entries before returning:
   - runner-scoped entries whose `runner_id` is not currently attached
     are copied back to `runner_offline_stream:{rid}` and XACKed from
     the connection stream so they do not spin in the live PEL.
   - connection-scoped entries (`revoke`, `force_refresh`) are always
     returned.
   - attached runner entries are returned normally.

Response body:

```json
{
  "messages": [
    {
      "stream_id": "1714080000-0",
      "mid": "...",
      "runner_id": "<runner_id_a>",
      "type": "assign",
      "body": { ... existing ServerMsg body ... }
    }
  ],
  "server_time": "...",
  "long_poll_interval_secs": 25
}
```

`messages` is empty on timeout. `stream_id` is monotonic within
`connection_stream:{cid}` (Redis Streams guarantee). `runner_id` is
nullable: present for runner-scoped frames, null for connection-scoped
frames like `revoke` / `force_refresh`. The daemon acks via the **next
poll's** `ack` list (decision #9).

ServerMsg-type mapping:

| Current frame         | `messages[i].type`                                                                |
| --------------------- | --------------------------------------------------------------------------------- |
| `Welcome`             | Returned synchronously from `POST .../sessions/` and `.../attach/`, not via poll. |
| `Assign`              | `assign`                                                                          |
| `Cancel`              | `cancel`                                                                          |
| `Decide`              | `decide`                                                                          |
| `ConfigPush`          | `config_push`                                                                     |
| `Ping`                | (gone; long-poll itself replaces it)                                              |
| `Revoke`              | `revoke`                                                                          |
| `RemoveRunner`        | `remove_runner`                                                                   |
| `ResumeAck`           | `resume_ack`                                                                      |
| (new) `force_refresh` | `force_refresh` (decision #17)                                                    |

### 7.4 Outbox semantics (Redis Streams + consumer group)

Backing store: **Redis Streams**, with three keying levels:

- **Stream**: one per connection, keyed `connection_stream:{cid}`.
  **Persistent across sessions** — created on first session open and
  never destroyed for the connection's lifetime (subject to PEL-aware
  sweeper-driven trimming; see retention contract below).
- **Consumer group**: one per connection, named
  `connection-group:{cid}`. Persistent across sessions. The group's
  PEL is the authoritative "delivered but not yet acked" record
  across the connection's history.
- **Consumer name**: per session, named `consumer-{sid}`. Changes on
  session eviction. Each consumer name has its own PEL slice within
  the group; entries owned by a particular consumer stay there until
  XACK or `XAUTOCLAIM` reassigns them.

This keying is why session handoff is implementable with ordinary
Redis primitives: `XAUTOCLAIM` operates within the single persistent
connection stream/group, reassigning ownership between consumer names
at `POST /sessions/` time (§7.1).

Implementation of `enqueue_for_runner` (replaces today's helper at
`apps/api/pi_dash/runner/services/pubsub.py:33`):

```python
def enqueue_for_runner(runner_id, msg):
    cid = connection_id_for_runner(runner_id)
    sid = active_session_id_for_connection(cid)
    if sid is None:
        # Offline policy (decision #18):
        # - run-binding messages like assign/cancel are not durable
        #   work queues for offline runners; the scheduler must
        #   re-match against an attached runner instead.
        # - only connection-scoped control messages may queue offline.
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
    # Live connection: append to the connection stream. The session_id
    # is implicit (the consumer name `consumer-{sid}` determines who
    # reads it). NOTE: no inline MAXLEN trim — see retention contract
    # below. Trimming is sweeper-driven and PEL-aware to preserve the
    # at-least-once delivery guarantee.
    stream_id = redis.xadd(
        f"connection_stream:{cid}",
        {"runner_id": runner_id, **msg},
    )
    return stream_id
```

Offline-stream handoff on attach (covered in §7.2): copy
entries from `runner_offline_stream:{rid}` into
`connection_stream:{cid}` oldest-first, preserving the original
offline id in a metadata field; XDEL each from the offline stream
after successful XADD.

**Ack semantics:**

- `XREADGROUP <group> <consumer> ... STREAMS <stream> >` delivers
  only entries not yet seen by _any_ consumer in the group. Entries
  it returns are added to that consumer's PEL.
- `XREADGROUP <group> <consumer> ... STREAMS <stream> 0` re-fetches
  the consumer's PEL (entries previously delivered but never acked)
  — used by the first poll after session-open to drain the PEL
  claimed from a prior session.
- `XACK <stream> <group> <id1> [<id2> ...]` removes specific IDs
  from the consumer's PEL. **XACK is exact-id, not range** — the
  daemon's `ack` body field carries the explicit flat list.
- If the daemon crashes mid-handle, those ids are still in the PEL.
  On next session-open (new session_id), §7.1 reassigns them onto
  `consumer-{new_sid}` via paginated `XAUTOCLAIM` so the next poll
  re-delivers them via `XREADGROUP ... 0`. Application-level `mid`
  dedupe at the daemon side (per-instance LRU; see
  `daemon_module.md` §8) gates double-handling at the handler.

The PEL is the durable record of "delivered but not yet acked"
messages; its survival across worker restarts and session evictions
(via per-session consumer-name reassignment) is what makes the
at-least-once delivery property correct without exotic cross-stream
operations.

Retention / cleanup contract:

- **No inline `MAXLEN` trim on `connection_stream:{cid}`.** Redis ≤7.3
  trims by `XADD ... MAXLEN`/`XTRIM` unconditionally — entries
  referenced in a consumer-group's PEL are evicted from the stream
  even though the PEL still references them. After such a trim, the
  PEL's IDs survive but their payloads are gone, and a subsequent
  `XREADGROUP ... 0` re-fetch of the PEL returns the IDs with `nil`
  bodies. That breaks at-least-once redelivery. (Redis 7.4 added an
  `ACKED` flag to address this, but pi-dash targets Redis 6.2.7 per
  `CLAUDE.md`, so we avoid that primitive.) Trimming is therefore
  **sweeper-driven and PEL-aware** — see `sweep_old_streams` in §7.10
  for the exact algorithm.
- On session eviction or session-DELETE, the **stream itself is not
  destroyed** — the connection stream and its consumer group are
  persistent. What changes is the active consumer name. The old
  consumer name's PEL is retained until either:
  - The next session-open `XAUTOCLAIM` reassigns it onto the new
    consumer (§7.1), or
  - The retention sweeper (`sweep_old_streams`, §7.10) deletes idle
    consumer names that haven't been claimed within
    `2 × access_token_ttl_secs` of session eviction.
- Connection streams with no active session and no in-PEL entries
  for >24h are eligible for deletion by `sweep_old_streams` (the
  connection is effectively orphaned at that point).
- `RunMessageDedupe` rows older than 7 days are deleted by periodic
  cleanup.

### 7.5 Daemon → cloud (replaces ClientMsg events)

| Current frame       | New endpoint                                                                                    | Notes                                                       |
| ------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `Hello`             | `POST /api/v1/runner/connections/<cid>/sessions/<sid>/runners/<rid>/attach/` (per-runner, §7.2) |                                                             |
| `Heartbeat`         | gone — folded into the long-poll request body's `status[]` (§7.3)                               | per-runner status, drives stale-busy reaping                |
| `Accept`            | `POST /api/v1/runner/runs/<run_id>/accept/`                                                     | Body carries `workspace_state`.                             |
| `RunStarted`        | `POST /api/v1/runner/runs/<run_id>/started/`                                                    |                                                             |
| `RunEvent`          | `POST /api/v1/runner/runs/<run_id>/events/` (batched)                                           | Body `{"events": [{seq, kind, payload}, ...]}`.             |
| `ApprovalRequest`   | `POST /api/v1/runner/runs/<run_id>/approvals/`                                                  | Same shape as today's `_persist_approval`.                  |
| `RunAwaitingReauth` | `POST /api/v1/runner/runs/<run_id>/awaiting-reauth/`                                            |                                                             |
| `RunCompleted`      | `POST /api/v1/runner/runs/<run_id>/complete/`                                                   |                                                             |
| `RunPaused`         | `POST /api/v1/runner/runs/<run_id>/pause/`                                                      |                                                             |
| `RunFailed`         | `POST /api/v1/runner/runs/<run_id>/fail/`                                                       |                                                             |
| `RunCancelled`      | `POST /api/v1/runner/runs/<run_id>/cancelled/`                                                  |                                                             |
| `RunResumed`        | `POST /api/v1/runner/runs/<run_id>/resumed/`                                                    |                                                             |
| `Bye`               | `DELETE /api/v1/runner/connections/<cid>/sessions/<sid>/`                                       | Or simply stop polling; server times out after 2× interval. |

Every POST carries `Idempotency-Key: <message_id>` and is idempotent
on `(run_id, message_id)`.

Authorization rule for every `/runs/<run_id>/...` endpoint:

- Resolve `run = AgentRun.objects.select_related("runner__connection").get(id=run_id)`.
- Require `run.runner.connection_id == request.auth_connection.id`.
- If false, reject with 403 `run_not_owned_by_connection`.
- This check is part of the shared HTTP transport service layer, not
  left to each endpoint individually.

### 7.6 Session fencing (no two daemons on one connection)

Decision #16. A connection can have at most one active session. The
authoritative state is the `RunnerSession.revoked_at IS NULL` row.

- `POST /sessions/` evicts the prior session before creating a new
  one. Prior session row gets `revoked_at = now`,
  `revoked_reason = 'superseded'`.
- **Eviction signaling.** The evicting request publishes a Redis
  pub/sub message on channel `session_eviction:<cid>` with body
  `{old_sid, new_sid}`. Each in-flight poll task is structured as
  `tokio::select! { _ = xreadgroup_block, msg = pubsub.next() }`
  (Python: `asyncio.wait` over both); on receiving the eviction
  pub/sub for its `old_sid`, the poll cancels its `XREADGROUP` and
  returns `409 session_evicted` with body
  `{ "reason": "superseded_by", "new_sid": "<new_sid>" }`.
  The pub/sub channel is best-effort; if a worker missed the
  signal (e.g. just-restarted), the next poll's session-id check
  catches it because the row's `revoked_at` is set.
- **PEL handoff is done at session-open.** Pending entries belong to
  the old session's consumer name (`consumer-{old_sid}`) within the
  persistent `connection-group:{cid}` group on the persistent
  `connection_stream:{cid}` stream. `POST /sessions/` reassigns them
  onto `consumer-{new_sid}` via a paginated `XAUTOCLAIM` loop (§7.1);
  pagination is mandatory because a single `XPENDING ... COUNT N` call
  is bounded and a busy connection's PEL can exceed any chosen page
  size. The old consumer name is retained for `2 × access_token_ttl_secs`
  post-eviction so a just-restarted daemon can still be fenced cleanly;
  after that window, `sweep_old_streams` reaps it.
- Each subsequent poll / ack / attach / detach call validates
  `session_id` (URL path segment); a stale one gets `409 session_evicted`.
- At most **one in-flight poll request per session** is allowed. A
  second concurrent poll on the same `session_id` returns 409
  `concurrent_poll` and the daemon treats that as a local bug / logic
  error, not a retryable network event.

This prevents two daemons (e.g. operator forgot the old one was
running, or a stale process) from fighting over delivery.

### 7.7 Liveness, summarized

Per decision #7:

- **Connection liveness**: `RunnerSession.last_seen_at` is updated
  on every poll. A periodic sweeper task (§7.10) marks the session
  revoked with reason `idle_timeout` when `last_seen_at` is older
  than `2 × long_poll_interval_secs` (~50s).
- **Per-runner liveness**: each poll's `status[]` entry updates the
  corresponding `Runner.last_heartbeat_at` and runs
  `_reap_stale_busy_runs`. A separate sweeper flips `Runner.status`
  to `OFFLINE` when `last_heartbeat_at` is older than
  `runner_offline_threshold_secs` (50s) even when the runner's
  session is still active — this catches the "one runner crashed,
  siblings still polling" case.
- **Empty `status[]`** is valid only when the session currently has
  zero attached runners. If the session has attached runners, empty
  `status[]` is rejected as `400 missing_runner_status` rather than
  being accepted with an alert.

### 7.8 Server-driven force refresh

Decision #17. New ServerMsg: `force_refresh`.

```json
{
  "type": "force_refresh",
  "reason": "signing_key_rotation" | "admin_request" | "suspected_leak",
  "min_rtg": 12   // optional: refuse access tokens with rtg < this
}
```

When the daemon receives one, it:

1. Acks immediately via the next poll's `ack` map.
2. Calls `POST /api/v1/runner/connections/<cid>/refresh/` (the only
   refresh endpoint, §5.3) to mint a new access token. The refresh
   endpoint deletes the `RunnerForceRefresh` row on success, so the
   new access token's `rtg` will pass the `min_rtg` check.
3. Resumes polling with the new token.

This shortens the staleness window (§5.4) from "up to TTL" to "up
to one poll round-trip" when the cloud has reason to invalidate
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
at upgrade-mint time, not supplied by the client). The WS handshake
on the cloud accepts this ticket exactly once and rejects anything
else. The socket is **per-run, time-bounded** (closes when the run
ends), and has no business authenticating as the connection — the
ticket already encodes the authorization.

**Storage**: Redis key `ws_upgrade_ticket:{ticket_uuid}` →
`{run_id, stream, runner_id, expires_at}`, set with `EX 60`.
Consumed atomically by the WS handshake via `GETDEL`. Reuse → reject.

This pattern keeps the WS code paid-for and battle-tested without
re-introducing always-on stateful auth. v1 does not ship a use case
that needs it; the door stays open for live log tail and future
media streams.

### 7.10 Sweepers and protocol-version rejection

**Periodic sweepers** (Celery beat or Django management command run
on a cron interval; choose during implementation):

- `sweep_idle_sessions` (every 30s):
  `RunnerSession.objects.filter(revoked_at__isnull=True, last_seen_at__lt=now - 2*long_poll_interval_secs)`
  → set `revoked_at=now, revoked_reason='idle_timeout'`. Publish
  `session_eviction:<cid>` for each so any straggler poll task
  detects it.
- `sweep_stale_runners` (every 30s):
  `Runner.objects.filter(status=ONLINE, last_heartbeat_at__lt=now - runner_offline_threshold_secs)`
  → set `status=OFFLINE`. Does **not** revoke the runner; it just
  reflects current observation. Re-attach revives it.
- `sweep_old_streams` (every 5 min): three jobs.
  1. **Old-consumer reaping.** For each revoked session older than
     `2 × access_token_ttl_secs`, walk the consumer names that
     belonged to it (`consumer-{sid}`) and either reassign any
     still-pending entries to the successor consumer (via paginated
     `XAUTOCLAIM`) if one exists, or
     `XGROUP DELCONSUMER connection_stream:{cid} connection-group:{cid} consumer-{sid}`
     to release them. Usually the successor claimed them at
     `POST /sessions/` time already, so this is a fallback path.
  2. **PEL-aware trim of `connection_stream:{cid}`.** Because we do
     **not** trim inline (§7.4 retention contract), the stream grows
     until this sweeper trims its tail. Per stream:
     ```
     # Find the smallest still-pending stream id across the group.
     # XPENDING (no IDs) returns a summary [count, min_id, max_id, [consumer counts]].
     summary = XPENDING connection_stream:{cid} connection-group:{cid}
     min_pending_id = summary.min_id  # may be None if PEL is empty
     time_cutoff_id = ms_to_stream_id(now - active_stream_min_retention_secs * 1000)
     if min_pending_id is None:
         safe_cutoff = time_cutoff_id
     else:
         safe_cutoff = min(time_cutoff_id, min_pending_id - 1)
     XTRIM connection_stream:{cid} MINID <safe_cutoff>
     ```
     This guarantees no PEL entry is trimmed: `safe_cutoff` is always
     strictly less than the smallest pending ID. `MINID` mode is exact
     (not approximate) — no risk of over-trim.
  3. **Orphaned-stream deletion.** Delete connection streams whose
     connection has been revoked or is idle with `XLEN == 0` for >24h.
     Delete offline streams with `XLEN == 0` and idle-time > 24h.

  The persistent `connection_stream:{cid}` and `connection-group:{cid}`
  are **not** destroyed by job 1 — they belong to the connection, not
  the session.

- `sweep_run_message_dedupe` (daily): delete `RunMessageDedupe`
  rows older than `run_message_dedupe_ttl_secs` (7 days).

**v3 protocol rejection** (decision #14). Two surfaces:

- **HTTP path** (the new control plane). `POST .../sessions/`
  inspects an `X-Runner-Protocol-Version` header on the request.
  Missing or `< 4` → `426 Upgrade Required`, body
  `{"error": "protocol_version_unsupported", "minimum": 4, "upgrade_url": "..."}`.
  No other endpoint enforces this (they're only reachable after
  session-open succeeds).
- **WS path** (kept for the per-run upgrade ticket only, §7.9). The
  consumer's upgrade handler rejects daemons advertising
  `X-Runner-Protocol < 4` by sending a close frame with code 1008
  and reason `protocol_version_unsupported`. v3 daemons attempting
  the old WS upgrade path see this close immediately and surface
  it to the operator.

## 8. Ordering, idempotency, dedupe

- **Cloud → daemon**: monotonic Redis Streams ids within the
  connection stream. Daemon preserves per-runner handler ordering by
  routing frames through the existing per-runner mailboxes. Cross-runner
  ordering is not guaranteed (and never was — different runners are
  independent).
  Delivery is **at-least-once**: ack happens only after the daemon's
  per-runner handler completes (decision #21, ack-on-handle), so any
  in-flight crash or 5xx on the next poll causes redelivery. Daemon
  carries a per-instance inbound `mid` LRU (cap ~256, TTL ~5 min) to
  drop a redelivered message that has already been processed; cap
  is comfortably above any realistic in-flight window. Per-runner
  cursors are tracked via the consumer-group PEL on the cloud, not
  a client-supplied scalar.
- **Daemon → cloud**: each POST carries an `Idempotency-Key` header
  set to the runner-side `message_id`. The endpoint deduplicates on
  `(run_id, message_id)` using the `RunMessageDedupe` table (decision
  #19). Insert-first-wins under a unique constraint; duplicate insert
  means "already processed" and returns the stored success shape.
  Rows older than 7 days are periodically deleted. Stale duplicates
  after the run is terminal are ignored.
- **Cancellation race**: when a `cancel` is queued and the run
  finishes naturally before the daemon polls, the cancel is dropped
  on the next poll because the run is terminal — the existing logic
  in `consumers._finalize_run` covers the symmetric WS case and
  ports unchanged.

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
| `active_stream_min_retention_secs` | 3600    |
| `run_message_dedupe_ttl_secs`      | 604800  |

All exposed in Django settings (`apple_pi_dash/settings/common.py`)
so production can tune without code changes.

## 9.1 Throttling

These endpoints are not user-interactive APIs and should not inherit
default user throttles.

- `POST /sessions/<sid>/poll`: no coarse DRF rate throttle. Safety is
  enforced by protocol constraints instead:
  - one active session per connection
  - one in-flight poll per session
  - server-side timeout of 25s
  - optional abuse backstop: reject sustained poll loops faster than
    1 request / 5s for >3 consecutive requests with 429
    `poll_rate_exceeded`
- Upstream lifecycle/event POSTs: `RunnerConnectionRateThrottle`,
  keyed by `connection_id`, default budget sized for event batching:
  600 requests/minute burst, 300 requests/minute sustained.
- Enrollment / refresh endpoints keep tighter auth-sensitive throttles
  keyed by connection and remote IP.

## 10. Failure modes

| Symptom                                                   | Cause                                                                                               | Recovery                                                                                                                                                                                                                                                                                                                                                        |
| --------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Daemon gets 401 `access_token_expired`                    | TTL elapsed                                                                                         | Daemon refreshes silently, retries the request once.                                                                                                                                                                                                                                                                                                            |
| Daemon gets 401 `membership_revoked`                      | Minting user lost workspace access                                                                  | Daemon shuts down (its `Connection` was just revoked server-side; nothing to recover). Surface the reason in TUI/logs.                                                                                                                                                                                                                                          |
| Daemon gets 401 `refresh_token_replayed`                  | Old refresh token re-used after rotation (real leak or crash-window)                                | Daemon shuts down. Operator re-enrolls if legitimate. The Connection is already revoked server-side.                                                                                                                                                                                                                                                            |
| Long-poll returns network error                           | Transient                                                                                           | Daemon retries with exponential backoff capped at 30s.                                                                                                                                                                                                                                                                                                          |
| Cloud has a queued `cancel` but the run already completed | Race                                                                                                | Cancel is dropped on next poll (run is terminal). No-op.                                                                                                                                                                                                                                                                                                        |
| Cloud restart / ASGI worker recycle                       | Routine                                                                                             | Outbox is in Redis, not in worker memory. Next poll lands on a different worker and works unchanged.                                                                                                                                                                                                                                                            |
| Daemon gets 409 `session_evicted`                         | Another daemon opened a new session for this connection                                             | Daemon shuts down its loop. The displacing daemon now owns delivery. Operator-visible event in TUI/logs.                                                                                                                                                                                                                                                        |
| Daemon receives `force_refresh` message                   | Cloud invalidating access tokens before TTL                                                         | Daemon refreshes inline before the next poll, then resumes.                                                                                                                                                                                                                                                                                                     |
| Daemon crashes mid-handle of a poll msg                   | Process killed before handler completes (so before id enters next poll's `ack` list — decision #21) | Message stays in `consumer-{sid}`'s PEL. On daemon restart + new session, session-open `XAUTOCLAIM`s the PEL onto `consumer-{new_sid}` (paginated to full drain). The first poll under the new session uses `XREADGROUP ... 0` to re-fetch; per-instance inbound `mid` LRU on the daemon side dedupes if the prior daemon had partially handled (decision #21). |
| Per-runner sibling offline                                | One runner crashed; daemon polls but omits it from `status[]`                                       | After 50s without status, that specific runner flips to OFFLINE; siblings on the same connection keep working.                                                                                                                                                                                                                                                  |

## 11. Phased rollout

Each phase ships independently and leaves the system in a working
state. Phases 1–3 are additive on the cloud side; phase 4 flips the
default; phase 5 retires the always-on WS dial.

### Phase 1 — Cloud: refresh-token + access-token issuance

- Schema migration: rename `secret_hash` → `refresh_token_hash`, add
  `refresh_token_generation`, `previous_refresh_token_hash`, and
  `access_token_signing_key_version`.
- New endpoint: `POST /api/v1/runner/connections/<cid>/refresh/`.
  Implements §5.3 including the live `is_workspace_member` check and
  lazy revoke-on-deny.
- New `AccessTokenAuthentication` DRF class (§5.4).
- Enrollment endpoint upgraded to mint and return both tokens (§5.1).
- The existing `ConnectionBearerAuthentication` stays in place
  unmodified for the duration of phases 1–3, so the WS path keeps
  working.

**Done when**: a daemon can call the refresh endpoint and get a
fresh access token; access-token verification works on a sample
endpoint; revoking workspace membership and then refreshing yields
401 `membership_revoked` and the connection's `revoked_at` is set.

### Phase 2 — Cloud: sessions, attach, long-poll, streams outbox

- New `RunnerSession` model + migration (§6).
- New endpoints:
  - `POST /api/v1/runner/connections/<cid>/sessions/` — opens a
    session, evicts any prior active session (publishes
    `session_eviction:<cid>` pub/sub), reassigns the prior consumer
    PEL onto the new consumer name via paginated `XAUTOCLAIM`,
    returns synchronous Welcome.
  - `POST /api/v1/runner/connections/<cid>/sessions/<sid>/runners/<rid>/attach/`
    — per-runner Hello replacement (§7.2). Mirrors the
    `_apply_hello` + group-add + online-mark + drain flow from
    `consumers.py:336-363`.
  - `DELETE` on both above (clean detach / session close).
  - `POST /api/v1/runner/connections/<cid>/sessions/<sid>/poll`
    (POST, not GET — request body carries `ack` + `status[]`).
- Redis Streams outbox helpers:
  - `enqueue_for_runner(runner_id, msg)` → `XADD connection_stream:{cid} runner_id=<rid> ...` (live session) or offline-stream fallback per decision #18.
  - `read_for_session(sid, attached_rids, timeout_ms)` →
    `XREADGROUP GROUP connection-group:{cid} consumer-{sid} ... BLOCK timeout_ms STREAMS connection_stream:{cid} (0|>)`. The poll handler picks `0` when `session_pel_drained:{sid}` is unset (first read after session-open), `>` once it's set.
  - `ack_for_session(sid, [stream_id, ...])` → `XACK connection_stream:{cid} connection-group:{cid} <id1> [<id2> ...]`.
- Migrate `send_to_runner` to **dual-write**: push to the existing
  Channels group **and** to the Redis stream for the runner's active
  session (if any). This is the safety net during transition; either
  transport delivers.

**Done when**: a test client can open a session, attach a runner,
poll the new endpoint, receive a queued message, ack it via the next
poll, and observe that subsequent polls don't return it again.
Concurrent session-open evicts the prior session and returns
`409 session_evicted` to the displaced poll.

### Phase 3 — Cloud: HTTP endpoints for runner-upstream events

- Implement every POST endpoint in §7.5, each backed by the same
  handler functions that today's WS consumer dispatches to (extract
  the bodies of `on_run_started`, `on_run_event`,
  `on_approval_request`, etc. into shared services so both
  transports call the same code).
- Add `RunMessageDedupe` model + helper service. Idempotency on
  `(run_id, message_id)` is mandatory and DB-backed in v1.
- Add the run-ownership authorization guard shared by all
  `/runs/<run_id>/...` endpoints.
- Add `RunnerConnectionRateThrottle` and wire it to the new runner
  HTTP endpoints.

**Done when**: the entire WS-driven test suite has a parallel
HTTP-driven test suite that exercises every lifecycle transition
with identical expectations.

### Phase 4 — Daemon: switch the connection loop to HTTPS

- `runner/src/cloud/ws.rs`: keep the module, but the daemon's
  `ConnectionLoop` (`runner/src/daemon/`) no longer dials it. New
  module `runner/src/cloud/http.rs` implements:
  - Refresh on startup and ~5 min before access-token expiry.
  - `POST .../sessions/` to open a session.
  - Per-`RunnerInstance` `POST .../attach/` on session-open and on
    every fresh-session reconnect (mirrors today's per-runner
    Hello-on-reconnect at `runner/src/cloud/ws.rs:130-145`).
  - Long-poll loop carrying per-runner `ack` cursors and per-runner
    `status` vector.
  - POST helpers for every upstream event.
  - `force_refresh` handler that triggers an inline refresh.
- Bump `WIRE_VERSION` / `protocol_version` to **4**. Cloud rejects
  3 with `426 upgrade required` and a message pointing at the new
  daemon binary.
- TUI surfaces "polling" / "refreshing" / "session_evicted" state
  in place of the current "connected" socket indicator.

**Done when**: `cargo test --workspace` passes including new
integration tests that drive the daemon end-to-end against a fake
HTTP cloud; an end-to-end run completes against the real cloud over
HTTP.

### Phase 5 — Cloud: retire WS as control plane

- `send_to_runner` stops dual-writing — it pushes only to the
  outbox.
- The Channels consumer's hot path (`receive_json` for control
  messages) is removed. The consumer is kept for the per-run WS
  upgrade path described in §7.9, with its handshake gated on the
  upgrade ticket.
- Remove `ConnectionBearerAuthentication` from the new endpoints
  (it was only kept around to dual-stack during phases 1–3).
- `_apply_hello`, `_resolve_connection_runner`, group-add, and the
  per-runner online/offline transitions move from `consumers.py`
  into a shared service module callable from both the WS consumer
  (still used by the per-run upgrade ticket path) and the new
  `attach/` endpoint.

**Done when**: monitoring shows zero control traffic on the WS
endpoint over a 24h window; the WS endpoint logs nothing but
upgrade-ticket handshakes.

## 12. Test plan

- **Unit**: refresh-endpoint state machine (revoked, replayed,
  membership-lost, happy path); access-token verification (expired,
  bad signature, mismatched `rtg`); outbox enqueue/drain/ack
  semantics; idempotency dedupe.
- **Integration**: end-to-end Assign → Accept → RunEvent ×N →
  RunCompleted over HTTP; cancellation while polling; reconnection
  after server restart; refresh-then-resume after sleep > TTL.
- **Property/contract**: protocol-version 4 mismatch returns 426;
  protocol-version 3 daemon advertises and is rejected with the
  upgrade-required marker.
- **Security**: refresh-time workspace-membership recheck — kick a
  member, refresh fails, `Connection.revoke()` cascades, in-flight
  `AgentRun` is cancelled, pinned QUEUED runs lose their pin,
  affected pods are re-drained. Up-to-TTL staleness is asserted
  (a previously-issued access token continues to work for ≤TTL,
  not indefinitely).
- **Session fencing**: open a session for a connection, then open a
  second session for the same connection from another process. The
  first session's in-flight poll returns `409 session_evicted`; the
  second session's first poll inherits the evicted session's PEL via
  paginated `XAUTOCLAIM` and delivers any in-flight messages exactly
  once. Variant: prior PEL > 1000 entries — verify the loop fully
  drains before the first poll runs.
- **Force refresh**: queue a `force_refresh` ServerMsg, observe the
  daemon refresh inline before its next normal-cycle refresh.
- **Per-runner liveness**: simulate one of two attached runners
  going silent (omitted from `status[]` for >50s); confirm only
  that runner is marked offline; sibling continues working;
  stale busy-run reaping fires for the silent runner's
  in-flight runs.

## 13. Open questions

- **Outbox stream keying**: v1 now locks to one persistent stream per
  connection with `runner_id` as a field. Revisit only if per-connection
  hot spots show up in production; the primary upside of the current
  shape is that it keeps `XREADGROUP`, `XACK`, and `XAUTOCLAIM` on
  the narrow, boring Redis path.
- **Access-token signing**: HS256 is locked for v1. Revisit Ed25519
  only if a non-Django verifier becomes a real requirement.
- **Replay window for refresh-token rotation**: currently 1
  generation. If clock skew or network races cause spurious leak
  detections, widen to 2. Measure first.
- **Per-run WS upgrade ticket lifetime**: 60s in §7.9 is a guess.
  Tighten or loosen after the first real consumer ships.
- **Liveness alert threshold for "runner attached but omitted from
  status[]"**: the offline transition is locked; only the operator
  alert threshold needs tuning from production telemetry.

## 14. Out of scope for v1

- Live log streaming (the canonical use case for §7.9). Designed
  for, not built in v1.
- Multi-region cloud. The outbox is single-Redis; cross-region adds
  a replication story we don't need yet.
- Non-WS push transports (SSE, WebTransport). Long-poll is the v1
  control plane; per-run streams use the existing WS code.
- Pre-existing daemon migration. There is no production data; daemons
  re-enroll cleanly on first start of the new binary.
