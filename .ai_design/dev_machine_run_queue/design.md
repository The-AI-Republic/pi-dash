# Dev-machine AgentRun queue

Status: implementation-ready specification; implementation has not started.

Reviewed against additional transport, process and chat paths on 2026-11-13. See [review.md](review.md) for findings and evidence. The decisions are closed in [implementation.md](implementation.md), with the end-to-end checklist in [tasks.md](tasks.md). Read all three specification files for handoff; implementation.md governs concrete wire, storage and recovery contracts. The corrections below are design requirements, not claims that the current code already provides these guarantees.

Research baseline: `pi-dash` main at `24b546d2d`, 2026-11-13. Paths below are relative to the repository root. This design covers the open-source daemon and both Python and Rust backend implementations present in this checkout; it does not assert which backend is deployed in production.

## 1. Problem and intended behavior

Cloud dispatch assigns each AgentRun to one eligible runner. A machine with five registered runners therefore exposes up to five independent execution slots today, regardless of its hardware. Runner count is useful for identities, projects, credentials and working directories, but should not determine how many expensive issue executions the machine starts simultaneously.

Add a second queue inside the shared dev-machine daemon. Keep cloud selection and run-to-runner binding intact. The daemon accepts assignments for idle runners, reserves those runners, and starts their work in local arrival order as machine execution slots become available.

Example with five runners and a configured limit of two:

| Moment | Executing | Waiting locally | Unoccupied runners |
| --- | ---: | ---: | ---: |
| Before new assignments | 1 | 0 | 4 |
| After four assignments arrive | 2 | 3 | 0 |
| After one execution finishes and the next starts | 2 | 2 | 1 |

The cloud may assign another issue to the newly unoccupied runner; that assignment joins the local queue's tail. Waiting runs keep their original runner. The daemon does not choose a different runner or return a run to the pod just because its machine is at capacity.

Default: **three concurrent AgentRun executions per daemon**, independent of the number of registered runners. The TUI exposes a machine setting to change this limit.

## 2. Scope and explicit boundaries

### Included

- One FIFO queue shared by all runner instances in the daemon, across pods, projects, workspaces and agent kinds, including built-in engine runners receiving AgentRun assignments.
- Durable receipt, runner reservation, cancellation, reconnect and restart behavior.
- A configurable concurrency cap, applied live without restarting active work.
- Local and cloud visibility of waiting for machine capacity.
- Matching lifecycle behavior in Python and Rust backends.

### Not included

- Resource sampling, peak memory/CPU/disk statistics, automatic sizing, or hard OS resource quotas.
- Changing cloud selection, machine-aware cloud scheduling, work stealing, priorities, preemption, or queue reordering.
- Multiple issue runs owned simultaneously by one runner.
- Restoring the retired worktree pool or its status.
- Resuming interrupted executing agents after daemon death.
- A computer-wide coordinator across separate OS users, independently configured daemons or unrelated agent processes. The scope is the existing shared daemon and its registered runners. Multiple independent daemons would each have their own cap.

**Chat scope decision:** v1 limits cloud-assigned AgentRuns. Direct/local chat and cloud chat use separate execution paths and are not added to this queue. Existing same-runner chat exclusion remains. Label the setting “Concurrent issue runs,” not a total-process or total-memory limit. Counting active chat turns in the same budget is a separate extension requiring interactive waiting/cancellation semantics; idle chat runtimes must never consume a slot merely by existing. This boundary means the v1 cap alone cannot guarantee a hardware budget when users also run chats or other processes.

## 3. Findings in the current code

| Area | Evidence | Design consequence |
| --- | --- | --- |
| Cloud matching | `apps/api/pi_dash/runner/services/matcher.py`, `drain_pod` / `drain_for_runner`; `rust-api/crates/services/src/runner_sessions/live_drains.rs`, `drain_pod` | Cloud locks eligible idle runners, chooses a run for each, persists its runner and `assigned` status, then delivers an assignment. Keep this algorithm. |
| Busy reservation | Python `matcher.py`, `BUSY_STATUSES`; Rust `runner_sessions/guards.rs` | `assigned` already occupies a runner. A new top-level run status is unnecessary for local capacity waiting. |
| Independent local execution | `runner/src/daemon/supervisor.rs`, `spawn_instance_tasks`, `RunnerLoop`, `ServerMsg::Assign` | Each instance has its own `current_run`. Assignment currently spawns an `AssignWorker` immediately. No shared execution permit exists in this path. |
| Local busy rejection | `RunnerLoop::nack_assign` and assignment guards | Another issue or active chat on the same runner produces `AssignRejectedBusy`. Queue capacity exhaustion must not use that rejection: it is an accepted local wait. |
| Early heartbeat identity | Assignment stamps `CurrentRunSummary` with `preparing` before worker setup | Waiting must likewise expose `in_flight_run` immediately. The field means “held assignment,” not only “agent process executing.” |
| Transport acknowledgment | `RunnerLoop::run` sends `AckEntry` after handling the frame; `runner/src/cloud/http.rs` owns transport | A received assignment can be acknowledged long before completion. Persist queue ownership before transport acknowledgment. |
| Execution acceptance | `AssignWorker::handle_assign_inner` resolves the workspace, sends `Accept`, then spawns the bridge | Gate before workspace setup. Receiving a queued assignment must not send `Accept`. |
| Cloud `Accept` semantics | `apps/api/pi_dash/runner/views/run_endpoints.py`, `RunAcceptEndpoint`; Rust `api/src/runner_runs/run_endpoints.rs`, `run_accept_inner` | `Accept` changes status to `running`; `RunStarted` later records session metadata and start time. Preserve those semantics. |
| Retired queue endpoint | Python `RunQueuedEndpoint`; Rust `run_queued_inner`; `runner/src/cloud/protocol.rs`, `RunQueued` | `/queued/` now acknowledges and ignores the old worktree notice. Do not reuse it or revive `waiting_for_worktree`. |
| Reconnect recovery | Python `session_service.py`, `build_session_open_redeliver`, `reap_stale_busy_runs`; Rust `runner_sessions/session_service.rs` | Session open can redeliver assigned runs; matching reported in-flight IDs protect held runs. Recovery must reconcile durable local entries with cloud authority. |
| Offline/stall recovery | Python `runner/tasks.py`, `fail_runs_on_offline_runners`, `reconcile_stalled_runs`; Rust jobs `runner_runs/offline_recovery.rs`, `sweeps_chat.rs` | Offline expiry still applies. Agent-silence detection must distinguish fresh local waiting from an agent that actually stalled. |
| Shutdown | `runner/src/daemon/supervisor.rs`, shutdown drain; `runner/src/daemon/drain_journal.rs` | Current shutdown reports held runs failed with `DaemonRestart`; the journal stores terminal delivery retries, not a durable work queue. Separate waiting from started work. |
| Config/UI | `runner/src/config/schema.rs`, `DaemonConfig`; `runner/src/tui/app.rs`, `spawn_save_config`; `runner/src/service/reload.rs` | Existing config save can restart the service. Add a dedicated live update path for the concurrency setting. |
| Shared IPC | `pidash-ipc/src/protocol.rs`; `runner/src/ipc/protocol.rs` re-exports it | Extend the shared protocol for settings and queue summaries, with additive/defaulted fields. |

One existing comment in the cloud matcher says chat and issue work can run concurrently; the current daemon's assignment handler explicitly rejects an assignment during an active chat on that runner. The daemon guard is the behavior this design preserves. Tests must cover the actual guard rather than relying on that comment.

## 4. Ownership and invariants

1. Cloud owns assignment and terminal status. Local scheduling cannot change runner binding or resurrect a terminal run.
2. A runner holds at most one issue assignment, whether waiting, preparing or executing.
3. A machine execution permit is required before workspace preparation or agent startup.
4. All new starts use the same coordinator. Hot-added runners cannot create a separate pool.
5. A run waiting locally is cloud `assigned`, reports its run ID in heartbeats, and blocks further cloud assignment to its runner.
6. FIFO means the order in which this daemon durably accepts assignments, not cloud issue creation time or cloud dispatch order across independent connections.
7. Duplicate delivery of the same assignment attempt never creates another queue entry or another execution.
8. Release a permit only after the worker and its agent execution have stopped. A cloud terminal response alone does not prove local resource release.
9. Never replay potentially started work automatically after a crash. Recovery may resume only confirmed, never-started waiting assignments.
10. A decrease below the active count creates a temporary over-limit drain state; it kills no work and permits no new start until the count is below the new limit.

## 5. Local coordinator

Introduce a daemon-owned `MachineRunQueue`, constructed once in `Supervisor` and shared through `RunnerSpawnCtx` with every existing and newly activated runner. Use an actor owning scheduling state rather than independent semaphore waits: the actor can serialize persistence, deduplication, cancellation, FIFO order and live resizing together.

Proposed modules: `runner/src/daemon/run_queue.rs` and `run_queue_store.rs`. Names in this section are proposed APIs, not existing symbols.

State:

- `limit`, default 3.
- Monotonic persisted `next_sequence` and a queue revision.
- Entries keyed by `(cloud origin, runner_id, run_id, assignment_token)` with a unique sequence, received time, cloud/workspace identity and assignment payload. The runner reservation map still permits only one held executable attempt per runner; terminal outbox records from older attempts cannot reserve or mutate a newer attempt.
- Entry phases: `waiting`, `starting`, `executing`, `finishing`; `reconciling` is a recovery overlay that forbids startup until ownership is validated.
- Active permit holders, with cancellation handles and worker completion channels.
- Per-runner reservation map and a bounded terminal-delivery/tombstone mechanism for duplicate handling.

Separate the runner's **held assignment** from its **executing worker**. Today `CurrentRun` always carries a worker completion receiver, and cloud-chat guards check `current_run.is_some()`. Introduce an explicit held-assignment state (`receiving`, `waiting`, `starting`, `executing`, `terminal_pending`) and optional worker handle. All busy/chat guards consult the held reservation, not worker existence. A dropped completion channel is a worker failure, not proof that its subprocess exited.

Only `waiting` entries are eligible for normal admission. Persist `starting` before handing out a permit. A crash after this write is treated as potentially started, even if no agent was actually spawned; conservative failure is preferable to duplicated external work.

The coordinator admits the oldest ready entry while active count is below the limit. A disconnected or unreconciled entry is temporarily ineligible; it retains its sequence, but must not block healthy runners indefinitely. Among ready entries, order is strictly FIFO. Once revalidated, a skipped entry regains its original relative position among remaining entries.

Serialize admission validation in FIFO order with a bounded request deadline. A pending validation reserves an admission credit so another start cannot consume the same opening, but does not launch a worker or count as executing. On timeout, return the credit, mark the entry temporarily unready, and move on. On response, re-check the current limit and cancellation state in the actor before granting the actual permit; a limit decrease during the request may return the entry to waiting. In that case send `defer` with the matching admission ID and a newer revision for this not-yet-started attempt. Ordinary delayed waiting reports remain rejected. Never infer readiness from a successful HTTP status without reading the admission disposition.

Do not hold an actor lock across network requests, workspace cloning, agent execution, or synchronous disk I/O on the async runtime. Persist transitions through a serialized store and return completion to the actor. Cancellation and receipt must remain responsive while other workers execute.

### Permit lifetime

A permit covers workspace setup, agent startup, execution, approval/reauth waits while the agent is retained, and teardown. A waiting-for-human state releases the permit only if the execution worker has actually exited, as with other completion paths. A continuation that creates a new AgentRun joins the queue normally. Same-run resume paths must be audited so that any new worker reacquires admission rather than bypassing it.

Waiting has no workspace clone, bridge startup or agent watchdog timer. The daemon keeps only queue metadata and the assignment payload. This bounds expensive executions, not the subprocess/thread count of an agent's tools.

## 6. Assignment lifecycle

### Receive

1. Preserve same-runner execution/chat guards. A different held run still causes the existing busy rejection.
2. Check duplicate identity **before** the different-run busy guard. A repeated assignment for the held run is acknowledged idempotently and keeps its queue order.
3. Acquire an exclusive runner-lane reservation shared with local and cloud chat before awaiting storage. Persist the new `waiting` entry and sequence. Failure to persist means no durable receipt and no transport acknowledgment; roll back the tentative reservation, surface the storage error and retry via transport recovery. Do not hold a blocking mutex through disk I/O; retain a reservation token instead.
4. Set that runner's held run summary to `waiting_for_capacity`; report `in_flight_run = run_id` even without an agent process. Clear prior-run observability so stale events cannot be attributed to this assignment.
5. Acknowledge the assignment transport frame. Enqueue a new capacity-phase report, described below.
6. Ask the coordinator to admit ready entries. Even immediately admitted work goes through the same reservation path.

**Transport dedupe must change:** `HttpLoop::dispatch` currently records `mid` before mailbox delivery and inline-acknowledges any duplicate. For assignments, track `received/pending` separately from `durably accepted`. A duplicate of a pending receipt may be coalesced, but cannot be acknowledged until the store confirms receipt. Feed durable receipt completion back to the HTTP loop, including the message ID and all corresponding stream IDs. On failure, clear pending dedupe state so delivery can retry. Session-open redelivery has no stream ID and still requires the same durable admission path. Do not change control-message ACK behavior accidentally while making this change.

### Admit and execute

1. Validate the assignment still belongs to this runner and is actionable using the admission endpoint below. A network failure leaves it waiting; a terminal/canceled/stale assignment is removed or finalized appropriately.
2. Serialize cancellation-versus-admission in the coordinator, persist `starting`, and acquire a permit. Never hold a scarce permit while waiting indefinitely for cloud connectivity.
3. Change the local phase to `preparing` and start `AssignWorker`. Maintain the run's heartbeat identity throughout.
4. Preserve workspace setup and `Accept` ordering. `Accept` transitions the cloud row to `running`; `RunStarted` retains its current role when the bridge starts.
5. Worker teardown reports completion to the coordinator. Release the permit once local execution has stopped, even if terminal delivery needs a retry. Keep terminal-delivery bookkeeping separate from capacity accounting.

Cloud admission and local cancellation cannot be one transaction. If cancellation arrives after cloud admission but before startup, do not launch; finalize cancellation. If it arrives after startup, use the existing running cancellation path. This is the same distributed cancellation race present today, with an explicit local serialization point.

## 7. Cloud state and protocol

Keep the top-level state model:

| Local phase | Cloud AgentRun status | Runner occupied? | Machine permit? |
| --- | --- | --- | --- |
| Not delivered yet | `assigned` | Yes | No |
| Waiting for capacity | `assigned` | Yes | No |
| Preparing workspace | `assigned` | Yes | Yes |
| Accepted / agent starting | `running` | Yes | Yes |
| Executing / approval or reauth | Existing lifecycle status | Yes | Yes while worker retained |
| Terminal or fully stopped human-input pause | Existing lifecycle status | Existing rules | No |

Introduce the optional capability `machine_run_queue_v1`, advertised by the cloud session handshake. Implement it in both backend variants before enabling the new daemon behavior against that cloud.

### Capacity report and admission

Add the authenticated runner endpoint `POST /api/v1/runner/runs/<id>/capacity/`, with actions `waiting`, `admit`, `defer`, and `reconcile`, using the exact request/response and idempotency contract in implementation.md §3. Reuse runner authentication and ownership checks, but add an explicit active-session fence: ordinary lifecycle authorization currently proves runner identity, not that the caller owns the current polling session. Carry the session ID and reject evicted sessions before admission. Use consistent session/runner/run locking order with session-open and dispatch in both backends.

Add a typed request/response method on `RunnerCloudClient` for this endpoint. It cannot use `RunnerOut::send` or `post_run_lifecycle` as an admission oracle: they return `()` and discard successful response bodies; the WS variant confirms channel enqueue only. Admission, reconciliation and receipt decisions must reach the coordinator as typed outcomes. A duplicate admission request must revalidate current ownership, cancellation, session and attempt under lock and return a complete current decision, never only `{duplicate: true}` or a cached obsolete permission to proceed. Persist request IDs for retry; `RunnerOut::send` currently generates a new envelope ID for each call.

- `waiting` records `execution_phase = waiting_for_capacity`, local queued time, queue sequence, position, reported active count, configured limit and report revision. It leaves status `assigned`.
- `admit` locks the run, validates current runner ownership and status, checks cancellation/terminal state, and records `execution_phase = preparing`. It returns an explicit `proceed` decision; acknowledgment by itself is not permission to start. It does not mark the run `running`.
- `reconcile` returns authoritative status/ownership and whether a never-started entry may be retained. It makes no execution transition. Use this before allowing restored entries to run.
- Accepted reports only update matching assigned attempts. Add a nullable UUID `assignment_token` to AgentRun and mint it inside every new assignment transaction, including reassignments to the same runner. Include it in every assignment builder and session-open redelivery and echo it in these requests. Validate it under the row lock. A redelivery retains the same token; a new assignment gets a new one. Old daemons can ignore the additive field. Old assigned rows need token initialization under lock before new-daemon reconciliation/redelivery.
- Ignore older report revisions for the same attempt. Late waiting reports cannot overwrite preparing/running/terminal phases. Return explicit terminal, canceled or stale-assignment dispositions.

Persist a dedicated, nullable capacity-wait JSON object on AgentRun (same schema in Django and SeaORM), including phase, attempt token, last revision and server receipt time. Use server time for freshness; local time is informational. Clear it on requeue/new assignment and terminal transitions; `Accept` clears the waiting presentation while retaining any timing data needed for wait-duration metrics. Do not overload `agent_metadata`, which is already populated for agent session identity at run start.

Queue position is a snapshot, not a guaranteed ETA. Report position changes with coalescing, and refresh waiting phase through runner polls. Include only that run's position and machine totals in workspace-visible responses; do not leak other projects' prompts, issue identifiers or runner identities across workspace permissions.

Use the UUID token rather than `assigned_at`: existing assignment payloads do not contain that timestamp, and time precision should not define execution identity. Carry the token on new-daemon `Accept`, `RunStarted`, terminal events and durable terminal retries too; capacity fencing alone cannot stop a delayed terminal event from affecting a later attempt. Keep legacy tokenless behavior only for legacy senders, with explicit capability/version validation in both backends.

### Heartbeats and reapers

- Every waiting runner continues regular polling with its held run ID. Its cloud `assigned` row continues to exclude it from matching.
- Capacity waiting is normal and has no agent-silence timeout. Stall detection must exclude a fresh, same-attempt `waiting_for_capacity` report; clear stale prior-run agent snapshots on admission/receipt.
- Preparing and running retain applicable existing health checks. Do not invent agent events to keep a waiting run alive.
- Offline detection remains authoritative. A disconnected machine can still lose assignments under the existing offline grace. Its restored queue must discard runs the cloud has already finalized.
- Audit assignment-age limits, lease refreshes, cancellation redelivery, session-open recovery and terminal reconciliation in both backends. A fresh waiting report must not be treated as a missing execution; it must not grant immunity to genuinely offline machines.

Do not send the retired `RunQueued` notice or introduce new `WAITING_FOR_WORKTREE` rows.

## 8. Durability, reconnect and shutdown

Store queue entries and the managed terminal outbox in `data_dir/run_queue.sqlite3`, using the SQLite schema, FULL durability and daemon lifetime lock specified in implementation.md §§1–2 and §5. This replaces the initial JSON-file proposal. Payloads can contain prompts and repository details: do not log them, and delete them after terminal acknowledgment. Persistence failure stops new receipt/admission and surfaces a diagnostic; do not silently fall back to an in-memory queue.

### Connection loss without process restart

Retain queue order and held-run heartbeat identities. Existing executions follow current connectivity behavior. Do not start another waiting run without successful admission validation. On reconnect, reconcile assignments before resuming admission and apply cancellation/terminal dispositions first.

### Daemon restart

Load the store before runner sessions open, so restored waiting reservations are represented correctly. Begin with admission disabled for restored entries. Reconcile each entry with its cloud runner session before making it ready.

- Confirmed never-started `waiting` entries preserve sequence and binding.
- `starting` or later entries are potentially executed: use existing restart failure/reconciliation, never automatic replay. Any surviving child execution must be terminated or accounted for before enabling new permits; inability to verify teardown blocks admission rather than claiming free capacity.
- Missing/terminal/canceled/reassigned cloud rows are removed or cancellation-finalized as appropriate.
- A missing local waiting entry can be recovered by existing cloud assigned-run redelivery, but its original arrival order is unrecoverable. Treat it as newly received and surface a recovery diagnostic. A malformed store stays preserved in storage-blocked mode; do not silently ignore corruption, replace it, or start potentially executed work.

Graceful shutdown freezes admission first. Preserve never-started waiting entries without reporting `DaemonRestart` for them. Drain/fail managed started entries through the SQLite terminal outbox; only legacy attempts use the existing terminal journal. The current shutdown code walks held runs; it must be changed to distinguish these sets. A long outage can still cause cloud offline failure, so preservation is conditional on reconciliation, not an unlimited execution guarantee.

For queue-managed attempts, the queue store also owns the durable terminal outbox: atomically replace the executable entry with `terminal_pending`, containing the exact lifecycle payload, assignment token and stable idempotency key. Replay cannot make this entry eligible. Remove it after a typed terminal acknowledgment or authoritative reconciliation. The existing failure journal remains responsible only for legacy attempts; never dual-write a managed attempt into two independent journals. Successful completion, cancellation and failure all use this outbox, including normal worker paths, not just shutdown. Current worker lifecycle sends log and swallow transport errors, so this requires an explicit integration change.

The coordinator owns reservation clearing and worker completion for managed attempts; workers no longer independently clear `current_run` before the coordinator records terminal state. A verified stopped execution releases its machine permit even if its runner stays reserved pending terminal acknowledgment. Distinguish `held_count` from `active_count` in status.

For hard-crash process recovery, persist verified process identity (PID plus platform process-start identity, not PID alone) when a bridge is spawned and record confirmed exit. Use the managed-process guard, POSIX process groups / Windows Job Objects and durable receipt handshake selected in implementation.md §6 to prevent untracked survivors before admitting replacement work. Current child `kill_on_drop` and process-local exit watches do not establish a cross-restart guarantee. Full descendant resource isolation is outside scope, but verifying termination of the managed agent is a release gate. If verification is unavailable, expose recovery-blocked state and require resolution; do not silently start replacements.

## 9. Cancellation, pause, removal and fairness

- **Cancel waiting:** remove from admission, durably record cancellation delivery, clear the local reservation when safe, and send `RunCancelled`. No agent or workspace setup is started. Duplicate cancellation succeeds without another launch or terminal event.
- **Cancel executing:** keep the permit until execution actually stops; then release and admit the next run.
- **Runner deletion/revocation:** remove its waiting entry only after applying the authoritative cloud disposition. Never launch an orphaned assignment. A local stop cannot silently drop a cloud-owned run; persist terminal notification or retain the reservation pending reconciliation.
- **Dispatch pause:** retain current meaning of refusing new cloud work. Already accepted waiting assignments continue unless explicitly canceled. Do not silently turn a dispatch pause into an execution pause. If “pause this machine's queue” is desired later, model it separately from limit zero.
- **Chat:** a waiting assignment reserves its runner's working directory lane. New chat turns on that runner must be rejected as occupied or use existing alternative-runner routing; they cannot bypass that reservation and race later admission. Chats on other runners remain outside the v1 cap.
- **Pinning:** pod-selected and dedicated assignments share the same local FIFO after receipt. Cloud dedicated-run fairness and pin policies still govern selection before receipt; local scheduling adds no priority jump.
- **Fresh cloud arrival:** an existing waiting entry cannot be overtaken by a newly accepted ready entry when a permit frees.

One waiting assignment per runner naturally bounds queue length by the daemon's runner count. The implementation should enforce this invariant rather than introduce an unrelated large queue bound.

Replace the advisory local-chat exclusion in `ipc/chat.rs::try_begin_local_turn` with the same atomic lane reservation used by assignment receipt. Its current flag checks and `rx_in_flight` reads do not span a new persistence await. Cloud-chat warm and send paths must also consult that reservation. Snapshot/heartbeat updates are projections of reservation ownership, not the lock itself.

## 10. Configuration and user interfaces

Proposed config:

```toml
[daemon]
max_concurrent_agent_runs = 3
```

Use an integer in 1–65535, with missing value defaulting to 3. Reject zero, negatives, fractions and overflow; zero is not a pause switch. A limit above the registered runner count is valid and simply has no additional effect. Validation must be shared by config loading, IPC and TUI edits.

Add a daemon-level TUI control labeled **Concurrent issue runs**, with help text “Maximum issue runs executing across this machine's runners. Extra assignments wait locally.” It must be editable even when no runner is configured and must not appear as a separate setting per runner.

Add the dedicated IPC request `MachineRunLimitSet`, handled by the daemon: validate, persist through the existing config mutation lock, then update the coordinator. Reply with configured limit, active count, waiting count and whether the update was applied. If persistence fails, leave the effective value unchanged. If the daemon is offline, the TUI can persist the setting for next startup and must say that it is not yet live.

Increasing the limit immediately admits additional ready work. Decreasing it does not cancel or suspend active work: show, for example, “4 executing / limit 2; waiting for executions to finish.” Use the dedicated request rather than `spawn_save_config`'s restart path for a limit-only change. Mixed edits that still require restart must retain explicit restart behavior, not be mistaken for a hot update.

Extend shared IPC status with machine limit, active count, waiting count and revision, and each held run with phase, queued time and position. Additive fields need defaults for old clients. Avoid misusing `CurrentRunSummary.started_at` as execution start for waiting entries: expose a distinct queued timestamp and show no execution duration until actual start.

TUI runner rows and run details show **Waiting for machine capacity · #2**. Machine summary shows **2/3 executing · 4 waiting**. Cloud run/issue details show the same reason using the new nullable capacity object. Existing `dispatch_wait` describes cloud-queued pinned runs; preserve that distinction rather than pretending an assigned local wait is a cloud queue position. Stale reports show last-known status, not a current capacity claim.

## 11. Compatibility and rollout

1. Ship backend schema, capability advertisement, endpoints, serialization and reaper changes in Python and Rust. Old daemons retain their current immediate-start behavior.
2. Ship daemon storage, coordinator, lifecycle integration and IPC, with TUI support. Enable queuing only after the cloud advertises the capability. For older clouds, explicitly expose “local run queue unsupported by this server”; retain legacy execution behavior rather than silently claiming the limit is enforced.
3. Current configuration has one daemon `cloud_url` and one shared HTTP transport; different workspaces do not imply different clouds. Remove mixed-cloud scheduling from v1. Cache capability for that cloud, refresh it on session open/reopen, and propagate it through both `WelcomePayload` and the constructed `ServerMsg::Welcome`. Once a daemon has durably queued work, capability loss or an incomplete backend rollout must freeze new admissions with an explicit unsupported/reconnecting state; it cannot switch that queue to unbounded legacy execution. Legacy mode is only a startup choice with no managed queue entries. HTTP is the v1 supported queue transport; legacy WS requires a real request/response admission protocol before it can enable this capability.
4. Exercise restart/cancel/reaper scenarios in staging before enabling by default for supported servers. Default remains 3 once active.
5. On downgrade, an older daemon cannot safely interpret the queue store. Require draining/canceling held work before downgrade; the new daemon must expose outstanding entries and document the downgrade precondition.

No cloud matcher algorithm change is required. Cloud availability still means an unoccupied runner; machine execution capacity is a separate concept. This deliberately reserves work to a machine even when another machine in the pod becomes idle. Improved cross-machine utilization is deferred.

## 12. Implementation sequence

1. **Cloud contract:** capability, assignment-token/protocol fields and capacity-wait schema, ownership/attempt validation, waiting/admit/defer/reconcile endpoint actions, serializers, heartbeat phase and reaper guards in both backends. Keep legacy worktree endpoint behavior unchanged.
2. **Coordinator and store:** FIFO actor, durable receipt, attempt-aware dedupe, cancellation, active permit accounting, limit changes and crash-safe transitions. Add proposed modules under `runner/src/daemon/` and paths under `runner/src/util/paths.rs`.
3. **Daemon integration:** construct once in supervisor, thread through hot-add paths, intercept `Assign` before worker startup, preserve transport ACK ordering, adapt heartbeat state, distinguish waiting from started shutdown entries, and ensure completion/teardown releases permits.
4. **Recovery:** reconcile before startup, assigned redelivery, terminal-pending handling, cancellation replay, orphan-process handling, store corruption and offline expiry behavior.
5. **TUI and cloud visibility:** shared IPC additions in `pidash-ipc`, live settings handler, machine summary, per-run waiting presentation and frontend types. Do not trigger a service restart for a limit-only edit.
6. **Rollout validation:** Python/Rust contract parity, end-to-end daemon tests and upgrade/downgrade documentation.

## 13. Acceptance and test matrix

| Scenario | Required result |
| --- | --- |
| Five runners, one executing, limit two, four assignments | One starts; three wait; all five runners are reserved. |
| Default config, ten runners with work | At most three issue workers prepare/execute concurrently. |
| Multiple pods/workspaces and hot-added runners, including after a live limit edit | Every assignment uses the same daemon coordinator and its current effective limit. |
| Local FIFO | Controlled arrival A/B/C starts in that order as slots free; new arrivals cannot jump ahead. |
| Duplicate waiting/starting/executing delivery | One reservation, one queue position and at most one worker. |
| Same run ID with stale attempt token | Stale entry cannot validate or launch. |
| Cancel while waiting | No clone/agent launch; queue position updates; cancellation delivered idempotently. |
| Cancel races admission/start | Either never starts or follows executing cancellation; no permit leak. |
| Running cancel with slow process exit | Permit remains occupied until teardown completes. |
| Raise/lower limit live | Additional admission on raise; no preemption or new admission above reduced limit. |
| Config persistence failure | No reported successful setting change; current effective limit retained. |
| Queue persistence failure before ACK | No lost acknowledged assignment; diagnostic and transport recovery. |
| Crash after receipt before ACK | Redelivery deduplicates and keeps persisted FIFO sequence. |
| Crash around admission/agent startup | Potentially started entry is never automatically replayed. |
| Graceful restart with running and waiting work | Running follows restart failure policy; confirmed waiting restores in order. |
| Offline expiry while waiting | Cloud finalization wins; reconnect cannot launch that entry. |
| Fresh heartbeat with prolonged capacity wait | No false missing-run, assignment-stall or agent-silence failure. |
| Terminal/cancel report arrives before delayed waiting report | No status/phase regression. |
| Waiting run and chat on same runner | Reservation blocks new chat from taking the working directory lane. |
| Old/new cloud and daemon combinations | Explicit capability behavior; no use of retired worktree state. |
| Different workspace viewers | See only authorized run details and aggregate capacity; no cross-workspace queue contents. |
| Duplicate transport message before durable receipt, then disk failure | No inline ACK of the pending assignment; retry remains possible. |
| Local chat starts while assignment storage is delayed | Exactly one lane reservation succeeds; no shared working-copy execution. |
| Successful HTTP response with terminal/ignored/duplicate disposition | No admission without explicit current `proceed`; response body is consumed. |
| Old session attempts admission after session eviction | Admission rejected even with valid runner credentials. |
| Delayed terminal report from a superseded attempt | New assignment remains unchanged. |
| Completion channel closes before verified process exit | No premature permit release; teardown/recovery path owns the slot. |
| Capability disappears while local work is queued | No fallback that bypasses the configured limit. |

Use fake execution workers and controllable barriers to measure active preparation/execution count; do not rely on timing sleeps alone. Add backend contract tests for Python/Rust parity and UI tests for live setting changes, waiting labels and stale snapshots. Crash-injection tests should cover every persistence/ACK/admission boundary.

## 14. Future work

Collect per-run peak memory, CPU activity and disk growth, with clearly defined sampling and subprocess attribution, before considering automatic recommendations. A later controller can suggest or adjust the configured limit, but must preserve the same admission invariants. Hardware-aware cloud scheduling, shared chat admission, per-workspace fairness and a cross-daemon host budget are separate design extensions.

The first release provides a deterministic issue-run limit and understandable waiting behavior. It does not promise that three arbitrary agent runs will fit every machine; the user controls that value.
