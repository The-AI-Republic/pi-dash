# Implementation contract and handoff

Status: ready for implementation, 2026-11-13. No feature code has been implemented.

Read [design.md](design.md), then this contract, then [tasks.md](tasks.md). [review.md](review.md) records the source findings that motivated these requirements. This document fixes the remaining choices and is normative for wire formats, ordering, recovery and compatibility. The task list's unchecked boxes represent implementation work, not unresolved product decisions.

## 1. Fixed decisions

| Decision | Required implementation |
| --- | --- |
| Scope | All cloud-assigned issue AgentRuns served by one daemon; chats on other runners excluded. |
| Default | `daemon.max_concurrent_agent_runs = 3`; valid range 1–65535; shared validation and `u16` storage. |
| Selection | Cloud retains runner selection. The daemon never changes binding. |
| Scheduling | FIFO by durable local sequence among ready entries; temporarily unready entries retain their sequence. |
| Worker budget | Workspace preparation through verified teardown, including approval/reauth and model backoff while the worker remains alive. |
| Cloud status | Local capacity wait remains `assigned`; existing `Accept` drives `running`. |
| Transport | Queue v1 uses the current HTTP runner transport. Legacy WS never advertises support. |
| Storage | One SQLite database for queue entries, sequence/revision allocation and managed terminal outbox; choose `rusqlite` with its bundled SQLite feature, executed on a dedicated blocking worker. |
| Durability | WAL, `synchronous=FULL`, foreign keys enabled; one serialized writer; durable transaction commit precedes receipt ACK. |
| Machine ownership | Hold a separate `fs2` exclusive nonblocking lock on `run_queue.lock` for the daemon lifetime, before opening sessions. A second daemon for the same data directory exits with an actionable error. |
| Process recovery | Managed launch scopes with an independent guard process and verified exit receipts; uncertain teardown blocks admission, never silently frees slots. See §6. |
| Unsupported server | Legacy startup only with an empty managed store; explicitly display “limit not enforced: server unsupported.” Once supported, no runtime fallback to unbounded starts. |
| Resource automation | Excluded; no CPU/RAM/disk sampling or automatic sizing. |

SQLite replaces the first draft's suggested JSON file and two-file coordination. Add a matching Rust dependency using the repository's supported toolchain and commit the lockfile; dependency version selection is routine implementation work. The store schema uses `PRAGMA user_version = 1`. Unknown newer schema, failed integrity check or unusable storage enters `storage_blocked`; never erase the file or use an empty in-memory replacement.

## 2. Local state and transaction boundaries

Persist `queue_meta(next_sequence, next_revision, schema_version)` and entries with:

`cloud_origin`, `runner_id`, `run_id`, `assignment_token`, `sequence`, `queued_at`, `phase`, `assignment_payload`, `report_revision`, `process_scope_id`, `pending_terminal_payload`, `pending_terminal_message_id`.

The composite attempt identity is unique. Enforce one nonterminal held attempt per runner. Terminal-delivery entries are not executable and may coexist with a later attempt only after cloud reconciliation proves the old attempt is no longer owned. Keep dedupe tombstones for 24 hours after authoritative terminal acknowledgment, without prompts; an unknown redelivery always passes cloud validation before it can execute, so tombstone expiry is not permission to replay.

| Durable phase | Permit | Restart action |
| --- | --- | --- |
| `waiting` | None | Reconcile, preserve sequence, then eligible. |
| `starting` | Held | Never replay; close/verify process scope and finalize restart failure. |
| `executing` | Held | Same as starting. |
| `finishing` | Held until verified exit | Complete teardown, then deliver persisted outcome. |
| `terminal_pending` | None after verified exit | Retry exact terminal message; never execute. |

`receiving`, `validating`, `reconciling` and `recovery_blocked` are coordinator overlays. `receiving` holds an atomic lane reservation before the database transaction; failure releases the tentative reservation without ACK. Persist `starting` before any workspace command or process launch, even when immediate execution is possible. A canceled `waiting` entry transitions directly to `terminal_pending` in one transaction.

Serialize actor commands; offload database work to the dedicated worker and process its completion messages. A transition is not externally committed until the database completion returns. Other lanes may handle cancellation and heartbeat while that transaction is outstanding, but cannot reuse its runner or sequence. Only the coordinator clears a managed reservation and changes permit accounting.

Separate counters: `active` includes starting/executing/finishing and cleanup-uncertain scopes; `waiting` counts waiting entries; `held` counts reserved runners. One in-progress validation holds an admission credit. Require `active + admission_credits < limit` before beginning another admission; v1 validates at most one entry at a time. A decrease below active use drains naturally.

Validation timeout is 5 seconds. Retry temporary failures after 1, 2, 4, 8, 16 and then 30 seconds, with up to 20% jitter. Retry timers use a monotonic clock. A failed candidate becomes unready until its deadline or a reconnect event; the next ready candidate can proceed. Use the actor's durable sequence, never HashMap order. Queue position is the one-based rank of all waiting entries, including temporarily unready ones, so it is informational rather than a promise that every earlier entry will start first.

## 3. Backend schema and protocol

Add these AgentRun columns in Django migrations and the matching Rust database migration/entity path:

- `assignment_token`: nullable UUID, indexed only if needed by existing query shape; mint UUIDv4 under the assignment transaction for every fresh binding, including binding again to the same runner.
- `assignment_protocol`: small integer, default 0; values 0 (legacy) and 1 (machine queue). Set from the assigning runner's negotiated capability. This is immutable for an assignment attempt.
- `capacity_wait`: nullable JSON object, schema below.

Record negotiated queue support on RunnerSession and expose it to the matcher through the runner's current active session. Session open request and welcome response each have an additive `capabilities: string[]` containing `machine_run_queue_v1`. A new daemon enables managed admission only when both parties advertise it. Existing sessions default to protocol 0. Assignment builders, direct-assignment paths and redelivery must share token/protocol construction helpers.

Do not convert a running legacy attempt on upgrade. Let it finish through legacy lifecycle behavior, count its local worker against the limit, and route subsequent assignments through protocol 1. An assigned legacy attempt received by a newly capable daemon may be adopted only by `reconcile` under lock when it is still `assigned`, has not been accepted, and has no live legacy owner session. Adoption returns a fresh token and protocol 1 to replace its payload before receipt ACK; otherwise return `legacy_in_flight` and retain existing handling until that attempt finishes. Never adopt `running`.

### Capacity endpoint

`POST /api/v1/runner/runs/<run_id>/capacity/`, existing runtime bearer authentication and runner ID header, JSON body:

```json
{
  "action": "waiting",
  "session_id": "<uuid>",
  "assignment_token": "<uuid>",
  "revision": 12,
  "queued_at": "2026-11-13T18:00:00Z",
  "queue_position": 2,
  "active": 3,
  "limit": 3
}
```

Actions are `waiting`, `admit`, `defer`, `reconcile`. `session_id` is mandatory. Protocol-1 attempts require the exact token. `reconcile` may omit a token only for the legacy adoption case above. Revision is a durable unsigned 64-bit counter and mandatory for mutations; reject overflow/negative/fractional values. UUIDs and RFC3339 times are validated; position is positive, counts nonnegative, limit 1–65535. Enforce a 16 KiB body limit. `queued_at` is informational; server receipt time governs freshness.

Successful responses always include `decision`, `status`, `assignment_token`, `assignment_protocol`, and `revision` (null where unavailable). `proceed` also includes `admission_id` equal to the persisted idempotency UUID of that admit call. `Idempotency-Key` is mandatory for mutations. Return complete responses even on retry:

| Decision | Meaning and daemon action |
| --- | --- |
| `recorded` | Waiting/defer report stored or already applied; no permission to execute. |
| `retain` | Reconcile confirms this never-started assigned attempt; keep its sequence. |
| `proceed` | Admit validates this session and attempt; re-check local cancellation/limit, then persist starting before preparation. |
| `cancel` | Cloud has `cancel_requested`; persist cancellation, never start. |
| `terminal` | Cloud already terminal; clean local never-started entry, or stop an existing scope before freeing it. |
| `stale_assignment` | Token/owner no longer matches; never execute or finalize the replacement attempt. |
| `already_started` | Status is running/approval/reauth; a restored waiting entry cannot execute it. Reconcile as interrupted or currently owned execution. |
| `stale_report` | Older report revision; ignore locally, refresh from authoritative state. |
| `legacy_in_flight` | Legacy adoption refused; follow upgrade handling above. |

HTTP 400 is invalid input; 401/403 is existing auth/authorization failure; 404 is deleted run; 409 with `session_evicted` requires reopening a session before retry; 503 with `runner_state_locked` or transient infrastructure error uses bounded retry. A missing capability/route never implies `proceed`. Do not expose status or tokens to a caller that fails runner authorization.

`capacity_wait` contains `phase` (`waiting_for_capacity` or `preparing`), `assignment_token`, `session_id`, `revision`, `queued_at`, `reported_at` (server time), `queue_position` (null in preparing), `active`, `limit`, and `admission_id` (only preparing). `admit` stores preparing and returns proceed but leaves the run `assigned`. `defer` requires the exact current `admission_id` and a newer revision while still assigned; it clears that admission ID and returns to waiting. Use defer if the limit is lowered or local persistence fails after a successful admit. Delayed ordinary waiting reports cannot reverse preparing. An uncertain admit response may be retried with its same idempotency key; if retry still cannot resolve, keep the entry waiting and reconcile before a later admit.

`reconcile` with a durable local `waiting` phase can reset an abandoned preparing report only after session change, with proof that no `Accept` occurred (cloud remains assigned). It never authorizes execution by itself. Subsequent admit uses a new durable request ID and newer revision. After a new session, initialize local revision to at least the server's returned revision plus one before sending mutations.

Recheck current session, token, status and cancellation on every admit retry. Never return an old cached proceed after terminal/cancel/reassignment. A repeated current admit returns the same admission ID; a later request cannot overwrite it without defer or recovery reconciliation. This is deliberately stronger than the existing `{duplicate: true}` lifecycle convention.

### Locks, lifecycle and polling

Use the lock order **Runner → active RunnerSession → AgentRun** where all three are needed. Update session open to lock Runner before replacing its session; poll bookkeeping, capacity operations, matcher and relevant lifecycle mutations must follow the same order when acquiring multiple locks. Reuse the existing bounded transaction waits. No HTTP/Redis I/O or background dispatch happens while those locks are held. Dispatch effects remain after commit. Add a concurrent session-open/admit/poll/cancel test to both backend suites.

For protocol-1 attempts, all lifecycle writes—including Accept, Started, completion, cancellation, failure, approval and resume transitions—require the assignment token. Authentication plus current run ownership is still required. Require current session for admission/Accept/Started; terminal outbox retries may use the current reopened session with the original attempt token, since the attempt survives reconnect. Old tokenless requests cannot modify a protocol-1 attempt even if an old session token remains valid. Protocol-0 attempts retain their existing compatibility behavior. Do not base enforcement solely on a client-supplied version string.

Give the worker a typed `accept_for_execution` call. After workspace preparation, it must receive `proceed` for current ownership/session/token before spawning the agent; cancellation or ignored/terminal responses stop startup. Transport failure retries within the same permit until canceled or connectivity policy stops the attempt; it never launches optimistically. Terminal typed responses are `applied`, `already_terminal`, `stale_assignment`, or `cancel_required`; the latter converts a pending non-cancel outcome to cancellation. Stable message IDs live in the local outbox. No new terminal path can silently discard transport failures.

Runner polls add `held_assignment_token` and a defaulted `capacity` snapshot carrying the current phase/revision/counts. Validate token and session before persisting phase or allowing a reported in-flight ID to suppress reaping. During received-but-not-yet-durable state, report the held ID/token as `receiving` for missing-run protection but do not exempt it indefinitely from offline recovery. Never ACK until durable receipt. `receiving` is not a UI queue position.

Refresh waiting visibility through every normal poll, independent of `agent_observability_v1`. Coalesce position-change reports to at most one per runner per second. A capacity report is fresh for 90 seconds by server receipt time. Agent-silence reaping excludes fresh waiting reports matching current attempt; offline recovery remains unchanged. A stale capacity report is displayed as last known; it alone neither fails a run nor suppresses existing recovery.

## 4. Queue and transport integration

Add `ReceiptResult { message_id, stream_ids, disposition }` feedback from the coordinator to the HTTP loop. `pending` dedupe is in-memory only; `committed` means SQLite has committed the attempt. A duplicate pending message joins its pending stream-ID list and receives no ACK. Failure clears pending state. Successful commit ACKs every attached stream ID. A later duplicate still goes through attempt lookup if its message ID is new. A terminal tombstone or authoritative rejection can ACK without adding executable work.

Persist receipt sequence in the same transaction as the entry. Do not hold a borrowed runner/mailbox lock during disk or network waits. Session-open redelivery without stream IDs uses identical attempt lookup/reservation/adoption behavior. Preserve existing force-refresh and cancellation routing.

Introduce `RunnerLaneReservation` shared by assignment receipt, cloud chat and IPC local chat. Acquisition is a single atomic ownership operation with an opaque guard token; no check-then-await-then-set pattern. A waiting issue owns the lane without a worker. Release only through the coordinator after durable cancellation or confirmed lifecycle disposition; release chat ownership when its existing turn guard ends. Idle warmed chats own no execution lane and are torn down before issue preparation as today.

## 5. Concrete persistence and shutdown behavior

The database file is `data_dir/run_queue.sqlite3`; sidecars inherit the private data-directory permissions. Create directory/file with owner-only access on Unix and the existing per-user data-directory ACL policy on Windows. Store prompts only in executable entries; transactionally remove the assignment payload when replacing it with a terminal outbox entry. Database compaction is not a secure-erasure claim. Never include prompts or credentials in queue diagnostics.

On terminal outcome: first persist the exact outcome in `finishing`; stop/verify execution scope; transactionally change to `terminal_pending`; release the permit; deliver terminal message; clear the runner reservation and reduce the record to a tombstone only after authoritative acknowledgment. A cloud terminal event received before local exit does not bypass teardown. Cloud cancellation takes precedence while outcome delivery is unresolved. A terminal outbox retry never restarts an agent.

Graceful daemon shutdown freezes admission, preserves `waiting`, and requests teardown of starting/executing scopes. Managed outcomes use SQLite, not `pending_run_failures.json`. Legacy attempts still use the old journal. Do not block process shutdown forever on the cloud: persist terminal obligations and let the next daemon send them. The next daemon loads reservations and process records before opening runner sessions.

Runner reconfiguration/removal uses the same path: unstarted work is cancellation-finalized or reconciled against deletion, started work is torn down before replacement. Changing the cloud URL or machine identity while the store is nonempty is refused with instructions to drain/cancel first. Never replay a stored payload into a different cloud.

## 6. Selected process lifetime mechanism

Implement a small internal **managed-process guard** in the existing `pidash` binary, invoked through a hidden subcommand. This is an implementation component, not a new public product mode. It owns the launched agent/process scope; agent protocol stdin/stdout/stderr remain separate from its control channel. All issue-worker process launches, including workspace preparation and agent driver respawns, must use this scope or register a nested child before it executes. Direct chat retains its existing launch path.

The purpose is to close daemon-crash and spawn-record windows without guessing at PIDs. The guard uses a fresh random scope UUID and private control secret delivered through an inherited pipe, never a command-line secret. Launch steps:

1. Persist `starting` and the scope UUID. Launch the guard, which is forbidden to start work yet.
2. The guard creates its containment primitive and writes an owner-only scope record with schema version, scope UUID, guard identity and control endpoint. It sends `ready` and waits for `start` on the authenticated control channel. Control EOF before start exits with a durable `never_started` receipt.
3. The daemon durably records that scope's ready acknowledgment, then sends `start`. The guard records `launching` before launching work. All agent/workspace traffic is forwarded transparently; guard diagnostics never share protocol stdout.
4. Normal finish, cancellation, control EOF, or teardown request causes the guard to stop the owned processes, verify exit, and atomically persist a `stopped` receipt containing scope UUID and exit result before it exits. The coordinator accepts only the matching receipt/verified live acknowledgment, not merely loss of the pipe or worker completion.

Use separate scope records owned by the guard; SQLite stores their identities and consumes receipts. There is no two-writer SQLite access. Ordering is conservative: a receipt can exist before the daemon commits its consumption; consuming twice is idempotent. A missing receipt never implies that work stopped.

### Platform choices

- **Linux/macOS:** the guard stays outside a dedicated POSIX process group for managed work, in the same session so it can place tracked children into that group. A small child anchor owns that group identity, and all tracked agent/preparation children join it. The guard retains the anchor child identity (and does not reap it) through group cleanup so the numeric group ID cannot be reused during signaling. Request graceful stop, wait 5 seconds, then force stop and wait for verified direct-child exits. The guard survives a force-kill of the work group and writes the exit receipt after reaping the tracked children and anchor. Recovered guards authenticate scope identity before any signal; no process group is selected merely by a remembered numeric ID. Detached descendants that deliberately escape the group are outside the v1 hard-resource guarantee; the managed agent and tracked preparation children must be stopped. See [POSIX session/group semantics](https://www.man7.org/linux/man-pages/man3/setsid.3p.html).
- **Windows:** the guard owns a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and no breakaway permission. Create each managed process suspended, assign it to the job, then resume. If assignment fails, terminate the suspended process and fail startup; never run outside the scope. Request graceful stop, then terminate the job and verify tracked process handles are signaled before writing stopped. Use process handles rather than unverified PID signaling. See [Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) and [AssignProcessToJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject).

The coordinator's normal teardown deadline is 15 seconds. Expiry sets `recovery_blocked` and retains the capacity charge; it does not free the slot. On daemon restart, live authenticated guards are told to stop, never to resume the old issue. Read their matching stopped/never-started receipts and finalize `DaemonRestart`. Never replay a `starting` entry even if its guard says never started.

If the guard also died without a receipt, automatic cleanup may proceed only with verified process identity/containment handles. PID-only kill is forbidden. Implement platform identity adapters using Linux process start ticks, macOS process start time, and Windows process creation time/handles. Primary references: [Linux process stat](https://www.kernel.org/pub/linux/docs/man-pages/book/man-pages-6.15.pdf), [Apple process identity fields](https://github.com/apple/darwin-xnu/blob/main/bsd/sys/proc_info.h), [Windows process times](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes). A platform adapter returning unknown is a supported fail-closed result, not a reason to release capacity.

Recovery UX is fixed: show the affected run/scope and “Execution stop could not be verified; new issue starts are blocked.” Add `MachineRunQueueRecover` IPC / a TUI **Retry recovery** action that rechecks guards, receipts and verified process state. It never deletes a store, ignores an active process, or marks unknown as stopped. Whole-machine reboot can establish that prior processes are gone only when the platform adapter supplies a reliable changed-boot proof; if unavailable, retain the diagnostic rather than infer it from wall-clock arithmetic. There is no “force free slot” action in v1. This is deliberate conservative behavior for exceptional double-crash/corruption cases; normal daemon restarts are handled by the guard protocol.

The implementer may choose the library bindings for these native primitives, but may not replace the guard/receipt protocol with `kill_on_drop`, PID guessing, or dropping a Tokio task. Cross-platform tests below verify the contract; no resource sizing work is bundled into it.

## 7. API/UI details

Add to shared IPC `DaemonInfo` an optional `run_queue` object: `mode`, `limit`, `active`, `waiting`, `held`, `revision`, `blocked_reason`. Modes: `enabled`, `legacy_unsupported`, `reconnecting`, `storage_blocked`, `recovery_blocked`. Per-run status adds optional `execution_phase`, `queued_at`, `queue_position`; leave execution start time absent until actual start. Old clients ignore optional fields.

`MachineRunLimitSet { limit }` returns `MachineRunLimitResult { configured_limit, effective_limit, active, waiting, applied }`. The daemon serializes the config write with existing config mutation helpers, then updates the actor before replying. On write failure return an error and preserve both old values. On daemon crash after write, next startup uses the persisted value. `MachineRunQueueRecover` returns a fresh snapshot plus per-scope diagnostics without prompts/secrets.

TUI adds a machine-level section outside the per-runner editor. Limit-only edits do not call restart-and-verify. Cloud issue/run serializers add nullable `capacity_wait` containing presentation fields only: phase, position, queued time, last report time, active and limit. Do not expose admission IDs, session IDs or cross-workspace runner lists. Update API-client contracts, `apps/web_new` waiting UI and the current web AgentRun detail surface where each consumes the serializer. Cloud capacity waiting is a separate branch from cloud `dispatch_wait`; use “Waiting for machine capacity” and retain the usual cancel action, with no pin-release action introduced by this feature.

## 8. Handoff completion rule

The implementation agent owns code, migrations, protocol parity, tests and user-facing documentation through the entire checklist. Native library selection, module layout within the named components and precise frontend component extraction are ordinary engineering choices, not additional product approvals. If a supported platform cannot satisfy a process-scope guarantee, keep the feature disabled there with an explicit diagnostic and report it as incomplete; do not claim end-to-end delivery.

The design is complete when read with this contract. Implementation is complete only when the checks in [tasks.md](tasks.md) pass and all required platforms/backends are covered. No remaining test gate is being represented as an already-proven runtime property.
