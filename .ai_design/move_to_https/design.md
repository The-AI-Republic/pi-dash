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

| #   | Question                                                       | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| --- | -------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Replace WS as the always-on control plane?                     | Yes. Control traffic moves to HTTPS long-polling + per-request POSTs.                                                                                                                                                                                                                                                                                                                                                                                          |
| 2   | Keep the WS protocol code?                                     | Yes. Reserved for **per-run, opt-in, time-bounded** data-heavy streams (live log tail, future media). No always-on socket. Authentication for a WS upgrade is a one-shot ticket minted by the access-token-bearing daemon (§7.9).                                                                                                                                                                                                                              |
| 3   | Token shape                                                    | Two tokens. **Refresh token** (long-lived, hashed in DB, on-disk 0600 in the daemon's existing credentials file). **Access token** (~1h TTL, self-contained signed token, daemon holds in memory only). Replaces the current single `connection_secret`.                                                                                                                                                                                                       |
| 4   | Where does workspace-membership authorization happen?          | At the refresh endpoint, on every refresh. Access-token verification is a stateless signature/expiry check (no DB hit, no membership check). Authorization staleness is therefore bounded by access-token TTL (≤1h by default), not "live per request." Sensitive endpoints may opt into per-request live re-check (§5.4).                                                                                                                                     |
| 5   | What happens when refresh is denied because membership lapsed? | **Lazy revoke-on-deny.** The refresh endpoint calls `Connection.revoke()` on the failing path. That cascades to runners, cancels in-flight `AgentRun`s, drops pinned-runner pins, and re-drains pods so other runners can pick up the orphaned work. No separate sweeper job needed; eventual consistency is bounded by the access-token TTL.                                                                                                                  |
| 6   | Refresh-token rotation                                         | Rotate on every successful refresh. Store the previous generation; if a previous-generation refresh is presented after rotation, treat it as a leak: revoke the connection. Matches OAuth2 best practice for installed clients.                                                                                                                                                                                                                                |
| 7   | Heartbeat / per-runner liveness                                | The poll **request body** carries a per-runner status vector `[{runner_id, status, in_flight_run, ts}, ...]`. The server applies each entry: updates `Runner.last_heartbeat_at`, runs the existing `_reap_stale_busy_runs` logic per entry. Connection-level liveness alone is insufficient — one surviving runner cannot vouch for dead siblings. The dedicated `Heartbeat` ClientMsg goes away; its fields move into the poll body.                          |
| 8   | Outbox backing store                                           | **Redis Streams**, one stream per `(session_id, runner_id)`. The daemon is a single consumer in a per-stream consumer group; reads use `XREADGROUP BLOCK 25s`, which **does not consume** messages — they remain in the pending entries list (PEL) until `XACK`. Acks come on the next poll's request body (§7.3). On session eviction, the stream is reassigned to the new session's consumer; pending entries are visible to the new consumer via PEL claim. |
| 9   | Ordering, dedupe, and ack model                                | Per-runner stream IDs are monotonic by Redis Streams construction. Daemon acks via `{runner_id: last_acked_id}` in the next poll body — **per-runner cursors**, not a single global cursor. The existing `Envelope.message_id` (mid) stays as the application-level dedupe key so handler-side retries are safe even when XACK is lost.                                                                                                                        |
| 10  | Connection-level session, per-runner attach                    | Long-poll runs at the **session** level (one session per connection). The session is created with `POST /sessions/`; runners then attach with per-runner `POST /sessions/<sid>/runners/<rid>/attach/`, which preserves today's per-runner Hello semantics: populates `authorised_runners`, marks the runner online, applies metadata, may resume in-flight. Detach is symmetric. See §7.1 and `consumers.py:336-363` for current behavior being preserved.     |
| 11  | Channel for `RunEvent`                                         | Batched POST `/api/v1/runner/runs/<run_id>/events/`, body `{"events": [...]}`. Daemon batches by time (≤ 250ms) **or** size (≤ 64 KB), whichever fires first. Phase 5 may upgrade per-run to a WS stream for runs flagged data-heavy.                                                                                                                                                                                                                          |
| 12  | Auto-issued `APIToken` at enrollment                           | Unchanged. Different threat model (interactive user CLI), independent revocation. Documented at §1 non-goals.                                                                                                                                                                                                                                                                                                                                                  |
| 13  | Pre-existing daemons                                           | None in production. The protocol described here is the only one shipped. The cloud serves the new endpoints from day one; the WS endpoint stays mounted but is no longer dialed by the daemon's main loop. Step-down of the WS endpoint from the control plane happens with the protocol-version bump in Phase 4.                                                                                                                                              |
| 14  | Protocol version                                               | Bump the cloud-acknowledged `protocol_version` (currently 3 in `apps/api/pi_dash/runner/views/register.py:20` and `runner/src/cloud/protocol.rs`) to **4**. The bump signals "control plane is HTTP; WS is opt-in per-run." Older daemons advertising version 3 are rejected with a clear error pointing at the upgrade path.                                                                                                                                  |
| 15  | TTLs                                                           | Access token: **1 hour**. Refresh token: **no fixed expiry** (revocable; tied to the Connection row). Long-poll timeout: **25 seconds** server-side (matches existing `HEARTBEAT_INTERVAL_SECS`). Daemon recovers by re-polling immediately on empty response.                                                                                                                                                                                                 |
| 16  | Session fencing                                                | Each connection has at most **one active session**. `POST /sessions/` evicts any prior session for the same connection: the prior `session_id` is marked revoked, its in-flight long-poll returns `409 session_evicted`, the new session takes over the runner streams (PEL claim). `session_id` is a required query parameter on every poll and ack call; mismatched session_id is rejected. Prevents two daemons fighting over one connection's outbox.      |
| 17  | Server-driven force refresh                                    | A `force_refresh` ServerMsg can be queued to make the daemon refresh its access token now (out-of-cycle). Use cases: signing-key rotation, suspected leak, admin-initiated re-authz before the natural TTL. Daemon treats it as a high-priority message: refreshes inline, then continues normal polling. Mirrors GitHub Actions' `ForceTokenRefresh`.                                                                                                         |

## 4. Conceptual model

| Concept                | What it is                                                                                                                                                                                                                                                                                                               |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Connection**         | Same as today: per-machine bond with `created_by` and `workspace`. The unit of trust the refresh token authenticates against.                                                                                                                                                                                            |
| **Refresh token**      | Long-lived credential the daemon stores at 0600 alongside the existing `credentials.toml`. Single-use rotation: each refresh consumes it and returns the next. Authenticates **against the user's current state**, not a snapshot.                                                                                       |
| **Access token**       | Short-TTL signed token (HS256 with a server-side secret, or Ed25519 — see §5.2). Self-contained: payload includes `connection_id`, `workspace_id`, `user_id`, `iat`, `exp`. Verified statelessly on every API request.                                                                                                   |
| **Session**            | Server-side row that owns delivery for a connection. Created by `POST /sessions/`; one active session per connection (newer evicts older, §7.6). Identified by `session_id` carried on every poll and ack. The session is the unit that "owns" pending messages and per-runner cursors.                                  |
| **Runner attach**      | Per-runner state inside a session. Created by `POST /sessions/<sid>/runners/<rid>/attach/` after the connection-level session exists. Replaces today's per-runner `Hello`: marks the runner online, populates server-side authorisation, applies metadata, may resume in-flight. Detach (or session eviction) cleans up. |
| **Long-poll**          | Daemon's only persistent activity. One open `GET /sessions/<sid>/poll` per session, ≤25s server timeout, returns 0..N pending control messages across attached runners. Request body carries the per-runner status vector (heartbeat + in-flight) and ack cursors.                                                       |
| **Outbox**             | Redis Streams, one stream per `(session_id, runner_id)`. `send_to_runner` does `XADD`; long-poll does `XREADGROUP BLOCK` (read-without-consume); ack does `XACK`. Survives worker restarts; pending entries survive session eviction via `XCLAIM`.                                                                       |
| **WS (legacy/opt-in)** | Existing `runner/src/cloud/ws.rs` + Channels consumer. Reachable only via the per-run upgrade endpoint (§7.9). Not used by the daemon's connection loop after Phase 4.                                                                                                                                                   |

## 5. Authentication

### 5.1 Token issuance

`POST /api/v1/runner/connections/enroll/` (existing endpoint at
`apps/api/pi_dash/runner/views/connections.py:210-298`) is extended:

- Continues to consume the one-time enrollment token.
- Returns **two** tokens instead of one `connection_secret`:
  - `refresh_token`: opaque random 32-byte base64. Hashed into
    `connection.refresh_token_hash` (renamed from
    `secret_hash`; see §6 for migration).
  - `access_token`: signed token with `exp = iat + 3600`.
- `connection.refresh_token_generation` (new column, default 0) is
  incremented to 1.
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

Payload:

```json
{
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

### 5.3 Refresh endpoint

`POST /api/v1/runner/connections/<connection_id>/refresh/`

Authentication: bearer the **refresh token** in the `Authorization`
header. No access token required (the daemon may have an expired
one).

Logic, in order, in a single transaction with `select_for_update` on
the Connection row:

1. Look up `Connection` by `id=connection_id, refresh_token_hash=hash(token)`.
2. If `connection.revoked_at IS NOT NULL` → 401 `connection_revoked`.
3. If the presented token's generation < `connection.refresh_token_generation - 1`
   → **leak detected**. Call `connection.revoke()`. Return 401
   `refresh_token_replayed`.
4. Live-check: `is_workspace_member(connection.created_by, connection.workspace_id)`.
   If false → **lazy revoke**. Call `connection.revoke()`. Return 401
   `membership_revoked`.
5. Mint a new refresh token. Store its hash. Increment
   `refresh_token_generation`. Mint a new access token with the new
   `rtg`.
6. Return both tokens.

The daemon writes the new refresh token to disk **before** discarding
the old one in memory, so a crash between step 6 and disk-write does
not strand the daemon. If the writer crashes after step 6 but before
disk-write, the next refresh will fail with `refresh_token_replayed`
and the connection self-revokes — recovery is to re-enroll. This is
the same crash-window OAuth2 clients accept; the rate is low enough
to ignore for v1.

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

Per-runner attach state lives in-memory inside the asgi worker for
the life of the active poll; durable per-session-per-runner state
that needs to survive worker restarts (e.g. last-acked stream id) is
recoverable from Redis Streams' consumer group PEL, so no DB column
is required for it.

Migration sequence (single migration since there is no production
data per `decisions #13` and prior fresh-install simplification in
`.ai_design/issue_runner/design.md`):

1. Add `refresh_token_generation`, `access_token_signing_key_version`.
2. Rename `secret_hash` and `secret_fingerprint`.
3. Existing `Connection` rows have `refresh_token_generation = 0` and
   are forced through re-enrollment on first daemon start (the daemon
   detects a credentials file lacking `[refresh]` and prompts).

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
  it to return `409 session_evicted`, claim its pending Redis Streams
  entries onto the new session via `XCLAIM`. Decision #16.
- Create a new `RunnerSession` row.
- Return the synchronous `Welcome` payload.

```
DELETE /api/v1/runner/connections/<cid>/sessions/<sid>/
```

Clean shutdown; server reaps the session and its streams. If the
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
    "welcome": { ... existing per-runner Welcome payload ... },
    "stream_id": "<redis stream key>",
    "starting_id": "0-0"   // or last-acked id from a prior session
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

Detach is symmetric:

```
DELETE /api/v1/runner/connections/<cid>/sessions/<sid>/runners/<rid>/
```

— marks the runner offline within this session. The runner row is
unchanged; another session/runner attach can revive it.

### 7.3 Long-poll (replaces cloud→daemon `ServerMsg` push)

```
POST /api/v1/runner/connections/<cid>/sessions/<sid>/poll?timeout=25
Authorization: Bearer <access_token>
Body — note this endpoint is `POST` rather than `GET`, because the
request carries `ack` cursors and per-runner status. (Some HTTP
intermediaries strip GET bodies; POST avoids the ambiguity.)
{
  "ack": {
    "<runner_id_a>": "<last_acked_stream_id>",
    "<runner_id_b>": "<last_acked_stream_id>"
  },
  "status": [
    { "runner_id": "<runner_id_a>", "status": "idle",
      "in_flight_run": null, "ts": "..." },
    { "runner_id": "<runner_id_b>", "status": "busy",
      "in_flight_run": "<uuid>", "ts": "..." }
  ]
}
```

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
4. For each `ack[]` entry: `XACK` the runner's stream consumer
   group at the given id. Pending entries ≤ that id are released.
5. For each attached runner, `XREADGROUP GROUP daemon-<sid>
consumer-<sid> COUNT 100 BLOCK 25000 STREAMS
runner_stream:{sid}:{rid_*} >`.
6. On any read returning, drain all available entries across all
   streams and return.

Response body:

```json
{
  "messages": [
    {
      "stream_id": "1714080000-0",
      "mid": "...",
      "runner_id": "...",
      "type": "assign",
      "body": { ... existing ServerMsg body ... }
    }
  ],
  "server_time": "...",
  "long_poll_interval_secs": 25
}
```

`messages` is empty on timeout. `stream_id` is per-runner monotonic
(Redis Streams guarantee). The daemon acks via the **next poll's**
`ack` map — per-runner cursor, not a single global scalar (decision
#9).

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

Backing store: **Redis Streams**, one stream per `(session_id, runner_id)`,
keyed `runner_stream:{sid}:{rid}`. Each session has a single consumer
group named `daemon-{sid}`. Single consumer per group.

Decision #4 in Codex's review: this fixes the "BLPOP destroys before
ack" bug and the "single scalar cursor over per-runner queues"
bug from the prior draft.

`send_to_runner` (today's helper at `apps/api/pi_dash/runner/services/pubsub.py:33`)
becomes:

```python
def enqueue_for_runner(runner_id, msg):
    sid = active_session_id_for_runner(runner_id)
    if sid is None:
        # No active session yet — runner is offline. Enqueue against
        # a "pending session" stream so the next attach picks it up.
        sid = pending_stream_id_for_runner(runner_id)
    stream_id = redis.xadd(f"runner_stream:{sid}:{runner_id}", msg)
    return stream_id
```

The long-poll endpoint reads with `XREADGROUP ... > ` (only entries
not yet delivered to _any_ consumer in the group). Pending entries
(delivered, not yet acked) survive worker restarts and survive
session eviction via `XCLAIM` from the new session's consumer.

**Ack semantics (corrected from the prior draft):**

- `XREADGROUP` does **not** consume — it moves entries to the
  pending entries list (PEL).
- `XACK` (called from the _next_ poll's `ack` map) removes them
  from the PEL.
- If the daemon crashes mid-handle, the PEL still holds the
  message; on next poll (same session) we replay it via
  `XREADGROUP ... 0` to re-fetch pending. On session eviction, the
  new session claims the PEL with `XCLAIM`.
- Application-level `mid` dedupe (decision #9) lets the handler
  ignore the replay if it already processed.

This is a real "messages-retained-until-acked" model, not the prior
draft's broken `BLPOP+LREM`.

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

### 7.6 Session fencing (no two daemons on one connection)

Decision #16. A connection can have at most one active session. The
authoritative state is the `RunnerSession.revoked_at IS NULL` row.

- `POST /sessions/` evicts the prior session before creating a new
  one. Prior session row gets `revoked_at = now`.
- The evicted session's in-flight poll detects the eviction (e.g.
  via a Redis pub/sub signal it subscribes to with the same
  `BLOCK`) and returns `409 session_evicted` with reason
  `superseded_by=<new_sid>`.
- Pending entries from the old session are claimed by the new
  session via `XCLAIM IDLE 0 ... <pel_ids>` so messages-in-flight
  are not lost.
- Each subsequent poll/ack call validates `session_id`; a stale
  one gets the same `409`.

This prevents two daemons (e.g. operator forgot the old one was
running, or a stale process) from fighting over delivery.

### 7.7 Liveness, summarized

Per decision #7:

- **Connection liveness**: `RunnerSession.last_seen_at` is updated
  on every poll. If a connection has no active session for >50s,
  the connection is considered offline.
- **Per-runner liveness**: each poll's `status[]` entry updates
  the corresponding `Runner.last_heartbeat_at` and runs
  `_reap_stale_busy_runs`. A runner is offline if its
  `last_heartbeat_at` is stale even when the connection is alive
  — this catches the "one runner crashed, siblings still polling"
  case Codex flagged.
- **Empty `status[]`** in a poll signals _the daemon believes none
  of its attached runners are healthy enough to report on_. This
  is unusual and triggers a connection-level alert, not silent
  acceptance.

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
2. Calls `POST /sessions/<sid>/refresh/` (or `POST .../connections/<cid>/refresh/`)
   to mint a new access token.
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
runner_id)`. The WS handshake on the cloud accepts this ticket
exactly once and rejects anything else. The socket is **per-run,
time-bounded** (closes when the run ends), and has no business
authenticating as the connection — the ticket already encodes the
authorization.

This pattern keeps the WS code paid-for and battle-tested without
re-introducing always-on stateful auth. v1 does not ship a use case
that needs it; the door stays open for live log tail and future
media streams.

## 8. Ordering, idempotency, dedupe

- **Cloud → daemon**: per-runner monotonic Redis Streams ids. Daemon
  processes per-stream in order. Cross-runner ordering is not
  guaranteed (and never was — different runners are independent).
  Re-delivery on retry is safe because every consumer-side handler
  (`Assign`, `Cancel`, `Decide`, etc.) is idempotent on `(run_id, mid)`.
  Per-runner cursors are tracked via the consumer-group PEL, not a
  client-supplied scalar.
- **Daemon → cloud**: each POST carries an `Idempotency-Key` header
  set to the runner-side `message_id`. The endpoint deduplicates on
  `(run_id, message_id)` against a small bounded LRU stored alongside
  the run row. Stale duplicates after the run is terminal are
  ignored.
- **Cancellation race**: when a `cancel` is queued and the run
  finishes naturally before the daemon polls, the cancel is dropped
  on the next poll because the run is terminal — the existing logic
  in `consumers._finalize_run` covers the symmetric WS case and
  ports unchanged.

## 9. Timing & tunables

| Tunable                         | Default            |
| ------------------------------- | ------------------ |
| `long_poll_interval_secs`       | 25                 |
| `access_token_ttl_secs`         | 3600               |
| `event_batch_max_age_ms`        | 250                |
| `event_batch_max_bytes`         | 65536              |
| `outbox_grace_window_messages`  | 1 (one full cycle) |
| `runner_offline_threshold_secs` | 50                 |

All exposed in Django settings (`apple_pi_dash/settings/common.py`)
so production can tune without code changes.

## 10. Failure modes

| Symptom                                                   | Cause                                                                | Recovery                                                                                                                                                             |
| --------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Daemon gets 401 `access_token_expired`                    | TTL elapsed                                                          | Daemon refreshes silently, retries the request once.                                                                                                                 |
| Daemon gets 401 `membership_revoked`                      | Minting user lost workspace access                                   | Daemon shuts down (its `Connection` was just revoked server-side; nothing to recover). Surface the reason in TUI/logs.                                               |
| Daemon gets 401 `refresh_token_replayed`                  | Old refresh token re-used after rotation (real leak or crash-window) | Daemon shuts down. Operator re-enrolls if legitimate. The Connection is already revoked server-side.                                                                 |
| Long-poll returns network error                           | Transient                                                            | Daemon retries with exponential backoff capped at 30s.                                                                                                               |
| Cloud has a queued `cancel` but the run already completed | Race                                                                 | Cancel is dropped on next poll (run is terminal). No-op.                                                                                                             |
| Cloud restart / ASGI worker recycle                       | Routine                                                              | Outbox is in Redis, not in worker memory. Next poll lands on a different worker and works unchanged.                                                                 |
| Daemon gets 409 `session_evicted`                         | Another daemon opened a new session for this connection              | Daemon shuts down its loop. The displacing daemon now owns delivery. Operator-visible event in TUI/logs.                                                             |
| Daemon receives `force_refresh` message                   | Cloud invalidating access tokens before TTL                          | Daemon refreshes inline before the next poll, then resumes.                                                                                                          |
| Daemon crashes mid-handle of a poll msg                   | Process killed before XACK                                           | Message stays in PEL for the consumer group. On next poll (same session), `XREADGROUP ... 0` re-fetches it; application-level `mid` dedupe prevents double-handling. |
| Per-runner sibling offline                                | One runner crashed; daemon polls but omits it from `status[]`        | After 50s without status, that specific runner flips to OFFLINE; siblings on the same connection keep working.                                                       |

## 11. Phased rollout

Each phase ships independently and leaves the system in a working
state. Phases 1–3 are additive on the cloud side; phase 4 flips the
default; phase 5 retires the always-on WS dial.

### Phase 1 — Cloud: refresh-token + access-token issuance

- Schema migration: rename `secret_hash` → `refresh_token_hash`, add
  `refresh_token_generation` and `access_token_signing_key_version`.
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
    session, evicts any prior active session (`XCLAIM` carry-over),
    returns synchronous Welcome.
  - `POST /api/v1/runner/connections/<cid>/sessions/<sid>/runners/<rid>/attach/`
    — per-runner Hello replacement (§7.2). Mirrors the
    `_apply_hello` + group-add + online-mark + drain flow from
    `consumers.py:336-363`.
  - `DELETE` on both above (clean detach / session close).
  - `POST /api/v1/runner/connections/<cid>/sessions/<sid>/poll`
    (POST, not GET — request body carries `ack` + `status[]`).
- Redis Streams outbox helpers:
  - `enqueue_for_runner(runner_id, msg)` → `XADD runner_stream:{sid}:{rid}`.
  - `read_for_session(sid, attached_rids, timeout_ms)` →
    `XREADGROUP ... BLOCK timeout_ms STREAMS ... >`.
  - `ack_for_session(sid, {rid: stream_id})` → `XACK ...`.
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
- Idempotency on `(run_id, message_id)` enforced via a per-run
  bounded LRU table or a small `RunMessageDedupe` table (TBD; v1 may
  use the AgentRun row itself with a JSON column of recent mids).

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
  `XCLAIM` and delivers any in-flight messages exactly once.
- **Force refresh**: queue a `force_refresh` ServerMsg, observe the
  daemon refresh inline before its next normal-cycle refresh.
- **Per-runner liveness**: simulate one of two attached runners
  going silent (omitted from `status[]` for >50s); confirm only
  that runner is marked offline; sibling continues working;
  stale busy-run reaping fires for the silent runner's
  in-flight runs.

## 13. Open questions

- **Outbox stream keying**: one stream per `(session, runner)` (chosen
  for v1) vs one stream per session with `runner_id` as a field. The
  per-(session, runner) shape lets each runner have an independent
  cursor naturally and keeps `XCLAIM` on session eviction cheap.
  Downside: one daemon with N runners makes one `XREADGROUP` call
  with N stream keys — fine up to ~hundreds of runners per
  connection, well above what `MAX_RUNNERS_PER_MACHINE = 50` allows.
  Revisit if we ever raise that ceiling.
- **Access-token signing**: HS256 (chosen for v1) vs Ed25519. If we
  ever want a sidecar (proxy, gateway) to verify tokens
  independently of Django, we'll move to asymmetric. Cost is one
  Rust dep and one cloud-side key-management story.
- **Replay window for refresh-token rotation**: currently 1
  generation. If clock skew or network races cause spurious leak
  detections, widen to 2. Measure first.
- **Per-run WS upgrade ticket lifetime**: 60s in §7.9 is a guess.
  Tighten or loosen after the first real consumer ships.
- **Liveness threshold for "empty status[]"**: §7.7 says an empty
  status vector is anomalous and triggers an alert. The exact alert
  threshold (one poll? three? a sustained 30s?) needs operational
  feedback before we wire it.

## 14. Out of scope for v1

- Live log streaming (the canonical use case for §7.9). Designed
  for, not built in v1.
- Multi-region cloud. The outbox is single-Redis; cross-region adds
  a replication story we don't need yet.
- Non-WS push transports (SSE, WebTransport). Long-poll is the v1
  control plane; per-run streams use the existing WS code.
- Pre-existing daemon migration. There is no production data; daemons
  re-enroll cleanly on first start of the new binary.
