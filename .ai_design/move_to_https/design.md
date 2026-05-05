# Runner ↔ Cloud Transport: HTTPS Long-Poll + Per-Runner Tokens

> Directory: `.ai_design/move_to_https/`
>
> Successor to the always-on WebSocket control plane currently in
> `runner/src/cloud/ws.rs` and `apps/api/pi_dash/runner/consumers.py`.
>
> This design replaces the old machine-level `Connection` model with a
> per-runner trust model:
>
> - one local machine may still host **N** runners
> - the daemon still supervises those **N** runners
> - but the cloud sees **N independent runner identities**
> - each runner has its own refresh token, access token, session, poll
>   loop, Redis stream, and revocation state
>
> The architecture is intentionally closer to GitHub Actions'
> self-hosted runner model than the earlier multiplexed-session design.
>
> The existing WebSocket protocol is **kept** only as a future opt-in
> transport for per-run, data-heavy streams such as live log tail or
> large event output. It is no longer the always-on control plane.

## 1. Goal

- Eliminate the always-on authenticated WebSocket as the control plane
  between cloud and daemon.
- Replace it with **per-runner HTTPS long-poll** for cloud→runner
  control messages and ordinary HTTPS POSTs for runner→cloud upstream
  events.
- Remove the `Connection` trust abstraction entirely. A `Runner` row
  becomes the unit of trust, auth, revocation, and delivery ownership.
- Preserve multi-runner-per-machine operation locally: one daemon still
  supervises N `RunnerInstance`s on one host, but it no longer owns a
  shared cloud identity or shared long-poll session.
- Keep the existing WS protocol/code available for future per-run,
  time-bounded upgrade streams.

Non-goals:

- Redesigning the runner ↔ codex/claude subprocess protocol.
- Building bulk runner enrollment UX in v1.
- Building live log streaming in v1.

## 1.1 Architectural layering

This migration changes the transport layer and the trust model. It does
not require a new message schema.

1. **Schema layer**
   `ClientMsg` / `ServerMsg` in `runner/src/cloud/protocol.rs` remain
   the canonical body schemas. HTTP request and response bodies reuse
   them.
2. **Call-site layer**
   Existing `RunnerOut::send(ClientMsg)` call sites and current
   cloud-side run lifecycle handlers remain conceptually unchanged.
   Transport routing changes underneath them.
3. **Transport layer**
   `runner/src/cloud/ws.rs` + Channels consumer are replaced by
   `runner/src/cloud/http.rs` + DRF endpoints for the control plane.
4. **Trust layer**
   The old `Connection` row disappears. The `Runner` row now carries
   user/workspace binding, refresh state, revocation state, and session
   ownership.

Four existing transport-specific frames stop being first-class
messages:

- `Hello` becomes session-open request/response metadata
- `Heartbeat` becomes the poll request body's `status`
- `Bye` becomes `DELETE /sessions/<sid>/`
- `Ping` disappears; long-poll timeout replaces it

One new cloud→runner control frame is added:

- `force_refresh`

## 2. Why now

The old design had three structural problems:

1. **Long-lived bearer with no real re-authorization boundary**
   The `connection_secret` was bound to a `Connection` row at mint
   time. After mint, no recurring membership check protected the daemon.
2. **WebSocket upgrade authenticated once, then trusted for hours**
   Even if HTTP auth were strong, the socket itself stayed open across
   a long blind spot.
3. **The cloud control plane was machine-multiplexed when the real
   worker is the runner**
   That forced the design to answer difficult questions about shared
   sessions, per-runner attach/detach, mixed liveness, mixed ack state,
   and shared outbox ownership.

The control plane does not need a permanent low-latency stream:

- approvals are human-paced
- assignments and cancels are discrete
- run lifecycle transitions are discrete
- heartbeat cadence is already coarse

The only likely high-volume flow is `RunEvent`, which is exactly the
kind of traffic that can later use a per-run WS upgrade.

## 3. Decisions Locked In

| #   | Question                                   | Decision                                                                                                                                                     |
| --- | ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | Replace WS as the always-on control plane? | Yes. Control traffic moves to per-runner HTTPS long-poll plus HTTPS POSTs.                                                                                   |
| 2   | Keep the WS protocol code?                 | Yes. Only for future per-run, opt-in, time-bounded data-heavy streams.                                                                                       |
| 3   | Trust unit                                 | `Connection` is dropped. `Runner` becomes the trust, auth, revocation, and delivery unit.                                                                    |
| 4   | Token shape                                | Per runner: one long-lived refresh token and one short-lived access token.                                                                                   |
| 5   | Workspace membership re-check              | Happens at refresh time for runner transport credentials.                                                                                                    |
| 6   | Refresh-token rotation                     | Rotate on every successful refresh. Keep one previous hash on `Runner` for replay detection.                                                                 |
| 7   | Liveness                                   | One poll loop per runner. Poll request carries that runner's `status`, `in_flight_run`, and timestamp.                                                       |
| 8   | Outbox backing store                       | Redis Streams: one persistent stream per runner, one persistent consumer group per runner, one consumer name per session.                                    |
| 9   | Ack model                                  | Exact stream-id ack list. `XACK` is by explicit IDs, not cursor ranges.                                                                                      |
| 10  | Session model                              | One runner = one session = one poll loop. No attach/detach sub-protocol. Session-open carries the metadata that `Hello` used to carry.                       |
| 11  | `RunEvent` channel                         | Batched HTTPS POST in v1. WS upgrade reserved for future heavy streams.                                                                                      |
| 12  | CLI credential                             | Separate machine-scoped `MachineToken`, not tied to runner transport credentials.                                                                            |
| 13  | Production migration complexity            | No production compatibility burden assumed; this design is the only one shipped.                                                                             |
| 14  | Protocol version                           | Bump to protocol version 4. Version 3 daemons are rejected.                                                                                                  |
| 15  | TTLs                                       | Access token 1h, long-poll timeout 25s, refresh token revocable with no fixed expiry.                                                                        |
| 16  | Session fencing                            | One active session per runner. New session evicts old session for that runner only.                                                                          |
| 17  | Force refresh                              | Per-runner `RunnerForceRefresh` directive plus `force_refresh` control message.                                                                              |
| 18  | Offline enqueue policy                     | `assign`, `cancel`, `decide`, `resume_ack` rejected while runner is offline; `config_push`, `remove_runner`, `revoke` may queue in a bounded offline stream. |
| 19  | Upstream idempotency                       | DB-backed `RunMessageDedupe(run, message_id)` unique constraint.                                                                                             |
| 20  | Throttling                                 | Runner-scoped throttles. Poll is protocol-bounded more than DRF-throttled.                                                                                   |
| 21  | Delivery semantics                         | At-least-once, ack-on-handle, with per-runner inbound `mid` dedupe.                                                                                          |
| 22  | Daemon networking shape                    | One auth/session state machine per `RunnerInstance`, but all of them share one daemon-level `reqwest::Client` transport pool.                                |
| 23  | On-disk credential layout                  | Per-runner credentials file under `runners/<rid>/credentials.toml`; machine token stored separately at machine scope.                                        |

## 4. Conceptual Model

| Concept         | What it is                                                                                                                                                                                        |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Runner`        | First-class trust and worker entity. Owns user/workspace binding, pod binding, refresh state, revocation state, and operational state.                                                            |
| Refresh token   | Long-lived per-runner credential stored on disk at 0600. Rotated on every successful refresh.                                                                                                     |
| Access token    | Short-lived per-runner bearer token, verified statelessly plus one indexed `rtg` check on `Runner`.                                                                                               |
| `MachineToken`  | Separate machine-scoped CLI credential for `pidash`, independent of runner transport.                                                                                                             |
| `RunnerSession` | Server-side row that owns delivery for one runner.                                                                                                                                                |
| Long-poll       | One open request per runner session, up to 25s, returning 0..N pending control messages for that runner.                                                                                          |
| Outbox          | Redis Streams: `runner_stream:{rid}`, `runner-group:{rid}`, `consumer-{sid}`.                                                                                                                     |
| Daemon          | Machine-local supervisor only: owns IPC, TUI bridge, startup validation, runner supervision, shared HTTP transport, and host-level resource coordination. It does not own a shared cloud session. |
| WS upgrade      | Future per-run ticketed stream, not the control plane.                                                                                                                                            |

## 5. Authentication

### 5.1 Enrollment and token issuance

`POST /api/v1/runner/runners/enroll/`

Request shape:

```json
{
  "enrollment_token": "et_...",
  "host_label": "my-laptop",
  "name": "..."
}
```

- `enrollment_token` — one-time bearer minted from the web UI.
- `host_label` — required. Used as part of the `MachineToken`
  bootstrap dedupe key `(user, workspace, host_label)`. Carries
  forward today's enrollment-request shape
  (`apps/api/pi_dash/runner/serializers.py:150`).
- `name` — optional; the daemon supplies a default.

Behavior:

- consumes the one-time enrollment token
- creates a new `Runner` row
- binds that runner to:
  - the enrolling user (existing `Runner.owner` field)
  - the workspace (existing `Runner.workspace` field)
  - the target pod / project context (existing `Runner.pod` field)
- mints:
  - `refresh_token`
  - `access_token`
- sets:
  - `refresh_token_generation = 1`
  - `previous_refresh_token_hash = ""`

Machine token bootstrap:

- if the enrolling user has no active `MachineToken` for the same
  `(workspace, host_label)`, the cloud also returns a `machine_token`
- otherwise the enrollment response omits it
- bootstrap runs inside the enrollment transaction: lock any existing
  live `MachineToken` row for `(user, workspace, host_label)` first,
  then mint only if none exists. The unique constraint is the safety
  net; the lock prevents the steady-state concurrent-enrollment race.

Operator-visible behavior:

- **Every new runner enrollment mints a fresh runner credential set.**
  Adding runner `R2` after `R1` means `R2` gets its own
  `refresh_token` + `access_token`; runner credentials are never
  shared across runners.
- **The CLI credential is not re-minted for every runner.** The first
  runner enrolled on a machine bootstraps the machine-scoped
  `MachineToken`; later runners on the same machine reuse that token.
- Therefore the system has **two token families** but normally only
  **one operator step**: enrolling a runner may also bootstrap the CLI
  token if the machine does not already have one.

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
  "machine_token": "mt_...",
  "machine_token_minted": true
}
```

### 5.2 Access-token format

Decision: HS256 for v1.

Key-ring contract:

- keys live in Django settings
- exactly one key is `active`
- zero or more keys may be `verify_only`
- the daemon never verifies access tokens locally

Example payload:

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

Verification order:

1. verify signature by `kid`
2. verify `exp`
3. verify `rtg >= runner.refresh_token_generation - 1`
4. if a `RunnerForceRefresh.min_rtg` row exists, require
   `rtg >= min_rtg`

### 5.3 Refresh endpoint

`POST /api/v1/runner/runners/<runner_id>/refresh/`

Auth:

- bearer refresh token
- no access token required

Algorithm:

1. lock `Runner` row with `select_for_update`
2. reject revoked runner
3. compare presented token hash:
   - current hash match: proceed
   - previous hash match: revoke runner, return
     `401 refresh_token_replayed`
   - neither: `401 invalid_refresh_token`
4. live-check `is_workspace_member(runner.owner, runner.workspace)`
   - false: revoke runner, return `401 membership_revoked`
5. rotate:
   - current hash → previous hash
   - set new current hash
   - increment `refresh_token_generation`
   - mint new access token
6. clear any `RunnerForceRefresh` row for this runner
7. return new tokens

Crash-window consequence:

- if the cloud rotated successfully but the daemon crashes before
  persisting the new refresh token, the next attempt presents the old
  token
- that old token now matches `previous_refresh_token_hash`
- the runner is revoked

That is acceptable in v1.

### 5.4 Authentication for every other runner-transport endpoint

`RunnerAccessTokenAuthentication`

Behavior:

- verify signature and `exp`
- load `Runner` by `sub`
- **reject if `runner.revoked_at IS NOT NULL`** → `401 runner_revoked`.
  `Runner.revoke()` does not bump `rtg`, so without this per-request
  check, an access token issued before revocation would survive up
  to its TTL. Mirrors the existing `revoked_at__isnull=True`
  predicate in `apps/api/pi_dash/runner/authentication.py:50-55`.
- enforce `rtg` lower bound (`rtg >= runner.refresh_token_generation - 1`)
- enforce any `RunnerForceRefresh.min_rtg`
- for runner-scoped endpoints, require URL `<runner_id>` to match
  `token.sub`
- for run-scoped endpoints, require `run.runner_id == token.sub`

This is **refresh-time authorization for membership** (live
membership re-checked at refresh time only) plus **per-request live
revocation** (the `revoked_at` check above). Membership-staleness is
bounded by access-token TTL unless the cloud forces an early
refresh; revocation takes effect on the next request.

### 5.5 What lives where on disk

```text
~/.config/apple-pi-dash-runner/
  daemon.toml
  machine_token.toml
  runners/
    <rid_A>/credentials.toml
    <rid_B>/credentials.toml
```

Per-runner credentials file:

```toml
[runner]
id = "..."
name = "..."

[refresh]
token = "rt_..."
generation = 7
issued_at = "..."
```

Machine token file:

```toml
[machine_token]
token = "mt_..."
issued_at = "..."
host_label = "..."
```

Rules:

- access tokens are never persisted
- each runner credentials file is logically owned by that runner
- in v1, the daemon process supervises and may create these files, but
  the design keeps them isolated so future subprocess ownership remains
  easy

### 5.6 MachineToken

`MachineToken` is the machine-scoped credential for the interactive
`pidash` CLI.

Properties:

- tied to `(user, workspace, host_label)`
- long-lived, separately revocable
- uses a different auth class than runner transport
- performs per-request workspace membership checks because it has no
  refresh chokepoint

This token is not part of the runner transport trust model.

Important distinction from runner enrollment:

- A machine may host N runners, but it should normally have only **one**
  active `MachineToken` per `(user, workspace, host_label)`.
- Enrolling runner `R2`, `R3`, ... on the same machine does **not**
  create additional CLI credentials; each of those enrollments only
  creates the new runner's transport credentials.
- If the CLI credential needs to be rotated independently, that is a
  separate operation (`pidash auth login` / revoke + reissue), not a
  side effect of adding another runner.

## 6. Data Model

### 6.1 Runner gains the trust fields

`Runner` already has the principal-binding fields today:

- `owner` (FK → User) — the trust principal. Assignment copies
  `runner.owner` onto `AgentRun.owner`
  (`apps/api/pi_dash/runner/services/matcher.py:209`); runner
  management permissions check it
  (`apps/api/pi_dash/runner/views/runners.py:21`,
  `views/runs.py:31`). The new auth model **uses this existing
  field**; no rename. §5.3 step 4 live-checks
  `is_workspace_member(runner.owner, runner.workspace)`.
- `workspace` (FK → Workspace) — already exists.
- `pod` (FK → Pod) — already exists.

`Runner` **gains** the auth/refresh fields formerly on `Connection`:

- `refresh_token_hash`
- `refresh_token_fingerprint`
- `refresh_token_generation`
- `previous_refresh_token_hash`
- `access_token_signing_key_version` (reserved for future use)
- `revoked_at`
- `revoked_reason`
- `enrolled_at`

Existing operational fields such as `status`, `last_heartbeat_at`,
`host_label`, `agent_versions` remain unchanged.

Drop: `Runner.connection` FK (Connection table is going away in
§6.2).

### 6.2 Connection table is dropped

`Connection` is removed entirely.

All trust and auth logic moves to `Runner`.

### 6.3 RunnerSession

`RunnerSession` is per-runner:

- `runner`
- `created_at`
- `last_seen_at`
- `revoked_at`
- `revoked_reason`
- `protocol_version`

Constraint:

- one active session per runner

Cascade rule:

- when `Runner.revoke()` runs, any active `RunnerSession` for that
  runner is revoked in the same transaction with
  `revoked_reason = "runner_revoked"`

### 6.4 RunnerForceRefresh

Per-runner row:

- `runner`
- `min_rtg`
- `reason`
- `created_at`

### 6.5 RunMessageDedupe

Unchanged:

- unique on `(run, message_id)`

### 6.6 MachineToken

Separate machine-scoped CLI credential model.

### 6.7 Pod / Project / Workspace

Unchanged relationship:

- `Pod` belongs to `Project`
- `Project` belongs to `Workspace`
- `Runner` belongs to `Pod`

No new pod ownership model is introduced.

### 6.8 Migration

Single migration sequence:

1. add trust/auth fields to `Runner`
2. rekey `RunnerSession` to `runner`
3. rekey `RunnerForceRefresh` to `runner`
4. drop `Runner.connection` FK
5. drop `Connection`
6. add `MachineToken`

## 7. Wire Protocol Mapping

The protocol is runner-bound. There are no machine-scoped or
connection-scoped control messages anymore.

### 7.1 Session lifecycle

`POST /api/v1/runner/runners/<rid>/sessions/`

Request body carries what `Hello` used to carry:

```json
{
  "version": "...",
  "os": "...",
  "arch": "...",
  "status": "idle",
  "in_flight_run": null,
  "project_slug": "...",
  "host_label": "...",
  "agent_versions": {}
}
```

Server behavior:

1. verify `token.sub == rid`
2. validate `project_slug` against `runner.pod.project.identifier`
3. evict any prior active session for this runner (mark
   `revoked_at`; publish `session_eviction:<rid>` pub/sub)
4. ensure:
   - `runner_stream:{rid}`
   - `runner-group:{rid}`
5. generate `new_sid`; claim old pending entries into
   `consumer-{new_sid}` via paginated `XAUTOCLAIM ... JUSTID`
6. **create `RunnerSession` row with `id=new_sid`** — this commits
   "I am the live session" before any subsequent step queries the
   active-session table. `enqueue_for_runner` (§7.4) decides
   live-stream vs offline-queue based on this lookup, so the row
   must exist before steps 8–10 run.
7. run the logic currently performed by per-runner `Hello`
   (`_apply_hello`); mark runner online
8. drain queued runs (`drain_for_runner_by_id` →
   `enqueue_for_runner` → `XADD runner_stream:{rid}` because the
   session created in step 6 is now live)
9. drain `runner_offline_stream:{rid}` into `runner_stream:{rid}`
10. if `in_flight_run` is present, resume it and prepare a
    `resume_ack` payload
11. return `session_id`, `welcome`, optional `resume_ack`

Ordering note: validating `project_slug` first (step 2) avoids
creating a phantom session row that has to be rolled back if
validation fails. The session row is the commit point; steps 7–10
only run after we have a live session for the runner.

`DELETE /api/v1/runner/runners/<rid>/sessions/<sid>/`

- clean shutdown for that runner
- persistent stream/group survive
- explicitly deletes `session_pel_drained:{sid}` as part of session
  teardown

### 7.2 No attach endpoint

The old multiplexed design needed attach/detach because one session
owned many runners.

That is gone.

Opening a runner session is attaching that runner.

### 7.3 Long-poll

`POST /api/v1/runner/runners/<rid>/sessions/<sid>/poll`

Request body:

```json
{
  "ack": ["1714080000-0", "1714080001-0"],
  "status": {
    "status": "busy",
    "in_flight_run": "<uuid>",
    "ts": "..."
  }
}
```

Server behavior:

1. verify active session
2. update `RunnerSession.last_seen_at`
3. update `Runner.last_heartbeat_at`
4. run `_reap_stale_busy_runs`
5. `XACK` any explicit ack IDs
6. `XREADGROUP`:
   - first poll after session-open uses `0`
   - later polls use `>`
7. return `messages[]`

`session_pel_drained:{sid}` lifecycle:

- the marker is set with `EX = 2 * access_token_ttl_secs`
- `DELETE /sessions/<sid>/` explicitly removes it
- session eviction explicitly removes the old session's marker
- expiry is the fallback if the daemon disappears mid-session

Response:

```json
{
  "messages": [
    {
      "stream_id": "1714080000-0",
      "mid": "...",
      "type": "assign",
      "body": {}
    }
  ],
  "server_time": "...",
  "long_poll_interval_secs": 25
}
```

Supported message types:

- `assign`
- `cancel`
- `decide`
- `config_push`
- `revoke`
- `remove_runner`
- `resume_ack`
- `force_refresh`

### 7.4 Outbox semantics

Redis Streams layout:

- stream: `runner_stream:{rid}`
- group: `runner-group:{rid}`
- consumer: `consumer-{sid}`

Offline queue:

- `runner_offline_stream:{rid}`
- `MAXLEN ~ 1000`
- `EXPIRE 86400`

`enqueue_for_runner(rid, msg)`:

- if runner has active session:
  - `XADD runner_stream:{rid} ...`
- else:
  - reject `assign`, `cancel`, `decide`, `resume_ack`
  - queue `config_push`, `remove_runner`, `revoke` in offline stream

Ack and redelivery semantics:

- `XREADGROUP ... >` delivers new entries and populates the PEL
- `XREADGROUP ... 0` re-reads this consumer's pending entries
- `XACK` removes exact IDs from the PEL
- on crash or restart:
  - next session-open `XAUTOCLAIM`s old pending IDs to the new
    consumer
  - first poll `0` replays them
  - per-runner inbound `mid` dedupe prevents double-handle

Retention rule:

- no inline `MAXLEN` trim on the live runner stream
- trimming is sweeper-driven and must not delete entries that still
  appear in any PEL

Reason:

- Redis versions in our target stack do not give us a safe enough
  inline trim primitive for pending entries
- delivery correctness matters more than an inline stream cap

### 7.5 Runner → cloud endpoints

Mappings:

- `Accept` → `/runs/<run_id>/accept/`
- `RunStarted` → `/runs/<run_id>/started/`
- `RunEvent` → `/runs/<run_id>/events/`
- `ApprovalRequest` → `/runs/<run_id>/approvals/`
- `RunAwaitingReauth` → `/runs/<run_id>/awaiting-reauth/`
- `RunCompleted` → `/runs/<run_id>/complete/`
- `RunPaused` → `/runs/<run_id>/pause/`
- `RunFailed` → `/runs/<run_id>/fail/`
- `RunCancelled` → `/runs/<run_id>/cancelled/`
- `RunResumed` → `/runs/<run_id>/resumed/`

Each carries:

- `Authorization: Bearer <access_token>`
- `Idempotency-Key: <message_id>`

Shared authorization rule:

- resolve run
- require `run.runner_id == request.auth_runner.id`

Lifecycle ordering contract:

- for a given `run_id`, `RunnerCloudClient` serializes non-event
  lifecycle POSTs and awaits each one before issuing the next:
  `RunStarted`, `RunPaused`, `RunAwaitingReauth`, `RunCompleted`,
  `RunFailed`, `RunCancelled`, and `RunResumed`
- different runs may POST concurrently
- `RunEvent` batching is independent and is not serialized behind
  lifecycle POSTs, so cloud handlers must tolerate `RunEvent`
  arriving before `RunStarted` for the same run

### 7.6 Session fencing

Per runner:

- at most one active session
- new session evicts old session for that runner only
- in-flight poll receives `409 session_evicted`
- concurrent second poll on the same session gets
  `409 concurrent_poll`

Signaling:

- pub/sub channel `session_eviction:<rid>`

Handoff:

- session-open uses paginated `XAUTOCLAIM` within the same
  `runner_stream:{rid}` / `runner-group:{rid}`

### 7.7 Liveness

Per-runner only:

- poll updates `Runner.last_heartbeat_at`
- sweeper flips runner offline after
  `runner_offline_threshold_secs`
- session liveness tracked independently via `RunnerSession.last_seen_at`

There is no machine-level heartbeat in the cloud transport.

### 7.8 Force refresh

Cloud:

- create or update `RunnerForceRefresh`
- queue `force_refresh` into `runner_stream:{rid}`

Daemon:

- receives `force_refresh`
- refreshes inline
- successful refresh clears the DB row

Revocation cleanup note:

- if `Runner.revoke()` runs because of membership loss, replay
  detection, or operator action, stream/group cleanup is scheduled
  shortly after the revoke path rather than happening inline
- this preserves a brief window for the daemon to observe shutdown
  cleanly while still bounding orphaned Redis resources

### 7.9 WebSocket reservation

Reserved endpoint:

`POST /api/v1/runner/runs/<run_id>/stream/upgrade/`

Returns a short-lived ticket that upgrades one run to a one-shot WS
stream for heavy traffic.

The ticket is:

- bound to `(run_id, stream, runner_id)`
- stored in Redis with `EX 60`
- consumed once via `GETDEL`

### 7.10 Sweepers and protocol rejection

Sweepers:

- `sweep_idle_sessions`
- `sweep_stale_runners`
- `sweep_old_streams`
- `sweep_run_message_dedupe`

Important `sweep_old_streams` rules:

- reclaim or delete old consumer names after the grace window
- trim runner streams only to a cutoff that preserves both:
  - **PEL entries** (delivered but not yet acked) — protected by
    the oldest still-pending stream id
  - **undelivered backlog** (XADDed but not yet returned by
    `XREADGROUP > `) — protected by the group's `last-delivered-id`
    from `XINFO GROUPS`. Anything past `last-delivered-id` is
    undelivered and must not be trimmed.
- algorithm:
  - read `last_delivered_id` from `XINFO GROUPS runner_stream:{rid}
runner-group:{rid}`
  - read `min_pending_id` from `XPENDING` summary (None if PEL is
    empty)
  - `safe_floor = (min_pending_id - 1)` if `min_pending_id` is set,
    else `last_delivered_id`
  - `safe_cutoff = min(time_cutoff_id, safe_floor)`
  - `XTRIM runner_stream:{rid} MINID <safe_cutoff>` (exact MINID,
    not approximate)
- delete empty orphaned streams after long idle periods

Why both bounds: `min_pending_id` alone is insufficient because
when PEL is empty the rule falls back to `time_cutoff_id`, which
can delete control messages that were XADDed but never reached an
`XREADGROUP > ` (e.g., during a daemon-hang window the cloud
continues to enqueue assignments while no consumer is reading).
Bounding by `last_delivered_id` ensures any undelivered ID is
preserved.

Protocol version check:

- `POST /sessions/` requires `X-Runner-Protocol-Version >= 4`
- older daemons receive `426 Upgrade Required`

## 8. Ordering, Idempotency, and Dedupe

- ordering is per runner, not cross-runner
- delivery is at-least-once
- daemon dedupes inbound redelivery by `mid`
- cloud dedupes runner→cloud POSTs by `(run, message_id)`

## 9. Timing and Tunables

| Tunable                            | Default |
| ---------------------------------- | ------- |
| `long_poll_interval_secs`          | 25      |
| `access_token_ttl_secs`            | 3600    |
| `runner_offline_threshold_secs`    | 50      |
| `offline_stream_ttl_secs`          | 86400   |
| `offline_stream_maxlen`            | 1000    |
| `runner_stream_min_retention_secs` | 3600    |
| `event_batch_max_age_ms`           | 250     |
| `event_batch_max_bytes`            | 65536   |
| `run_message_dedupe_ttl_secs`      | 604800  |

### 9.1 Throttling

- poll endpoint is protocol-bounded more than DRF-throttled
- upstream POSTs use runner-scoped token-bucket throttles
- refresh and enroll remain tightly auth-throttled

## 10. Failure Modes

| Symptom                                               | Recovery                                                                                                                      |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| one runner gets `401 access_token_expired`            | refresh that runner only and retry once                                                                                       |
| one runner gets `401 runner_revoked`                  | stop that runner only — server marked `revoked_at` mid-token-life; the per-request revoked check (§5.4) caught it             |
| one runner gets `401 membership_revoked`              | revoke and stop that runner only                                                                                              |
| one runner gets `401 refresh_token_replayed`          | revoke and stop that runner only                                                                                              |
| one runner sees network error                         | retry that runner's poll with backoff                                                                                         |
| one runner gets `409 session_evicted`                 | stop that runner; the new session owns delivery                                                                               |
| one runner receives `force_refresh`                   | refresh inline, continue                                                                                                      |
| daemon crashes mid-handle                             | next session reclaims and redelivers from PEL                                                                                 |
| daemon performs graceful shutdown with in-flight work | stop each runner's agent subprocess best-effort, emit final `RunCancelled` only when auth is still valid, then close sessions |

The key property is isolation: sibling runners on the same machine are
not transport-coupled.

## 11. Phased Rollout

Dual-stack invariant:

- during phases 2–4, both the legacy WS control plane and the new HTTP
  endpoints remain active on the cloud
- `send_to_runner` dual-writes to the Channels group and the runner's
  Redis stream during this window
- daemons continue using WS until Phase 4 flips them to HTTP
- only Phase 5 retires the WS control plane

### Phase 1 — Cloud auth and data-model shift

- add trust/auth fields to `Runner`
- drop `Connection`
- add `MachineToken`
- rekey `RunnerSession` and `RunnerForceRefresh`
- add enroll and refresh endpoints

### Phase 2 — Cloud per-runner sessions and Redis outbox

- add session open / close / poll endpoints
- add Redis Streams helpers
- add sweepers

### Phase 3 — Cloud runner→cloud HTTP endpoints

- extract WS handler bodies into shared services
- add HTTP lifecycle/event endpoints
- add idempotency handling

### Phase 4 — Daemon per-runner HTTP loops

- create shared daemon-level HTTP transport
- create one `RunnerCloudClient` and one `HttpLoop` per runner
- remove shared cloud session / shared demux / attach-emitter ideas
- implement per-runner single-flight refresh so concurrent refresh
  triggers collapse to one in-flight refresh operation per runner
- bump to protocol version 4

### Phase 5 — Retire WS as control plane

- stop using WS for control traffic
- keep WS only for future ticketed per-run streams

## 12. Test Plan

- refresh rotation and replay detection
- runner revocation on membership loss
- session open / poll / ack / replay
- session fencing per runner
- two concurrent runners on one daemon with isolation
- idempotent runner→cloud POST handling
- machine token bootstrap and revocation
- protocol version rejection

## 13. Open Questions

- whether to add bulk runner enrollment UX later
- whether to move from HS256 to Ed25519 later
- whether machine tokens should gain expiry in a future revision
- operational tuning for per-runner mailbox and poll backpressure

## 14. Out of Scope for v1

- live log streaming over WS
- SSE / WebTransport
- bulk enrollment API
- multi-region transport design
