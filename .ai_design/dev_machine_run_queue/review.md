# Code-grounded design review

Date: 2026-11-13. Baseline: `24b546d2d`. Reviewed artifact: [design.md](design.md).

Verdict: the two-queue architecture fits current cloud runner selection. The initial draft understated delivery, ownership and teardown work; those findings are now resolved at specification level by [implementation.md](implementation.md). [tasks.md](tasks.md) maps the remaining work to implementation and validation. No product code has been changed or runtime guarantees verified by this review.

Priority: P1 means a correctness requirement before enabling the queue; P2 means a necessary scope or compatibility correction. Findings concern gaps between the proposed design and existing code, not assertions that every existing path is already defective in normal use.

## Findings

### R1 — P1: delaying the runner ACK does not guarantee durable receipt

Evidence: [`HttpLoop::dispatch`](../../runner/src/cloud/http.rs:2003) calls `mid_dedupe.record` before sending to the runner mailbox. A repeated message is put straight into `inline_acks`. [`RunnerLoop`](../../runner/src/daemon/supervisor.rs:2544) separately sends its ACK after processing.

Failure scenario: an assignment reaches the mailbox, persistence is delayed or fails, and the same message arrives again. Transport dedupe can ACK that duplicate before any durable queue record exists. Merely moving the runner's ACK after the store write does not close this path.

Correction: distinguish pending receipts from durable receipts in HTTP dedupe; only ACK assignment duplicates after durable confirmation. Feed both message identity and stream ACK identities back from receipt. Cover session-open redelivery without stream IDs. Added to design §6 and tests.

### R2 — P1: existing lifecycle transport cannot return an admission decision

Evidence: [`post_run_lifecycle`](../../runner/src/cloud/http.rs:848) discards the response JSON and returns `()`. [`RunnerOut::send`](../../runner/src/daemon/runner_out.rs:56) returns transport success; its WS variant confirms only local channel enqueue. [`AssignWorker::send`](../../runner/src/daemon/supervisor.rs:4812) logs errors and continues. Python [`_record_dedupe`](../../apps/api/pi_dash/runner/views/run_endpoints.py:51) and lifecycle endpoints commonly return a successful duplicate or ignored response.

Failure scenario: implementing `admit` as another lifecycle message would allow a transport-success result to be mistaken for permission to start despite cancellation, terminal state or duplicate handling. A lost response followed by a duplicate response also needs a complete current decision.

Correction: a typed HTTP request/response method returns explicit admission/reconciliation dispositions. Revalidate duplicate admission requests instead of returning an empty duplicate ACK or stale cached permission. Persist stable request IDs for durable retries. Added to §7.

### R3 — P1: runner reservation must cover the persistence await and both chat paths

Evidence: [`try_begin_local_turn`](../../runner/src/ipc/chat.rs:375) explicitly describes advisory exclusion based on a chat flag and `rx_in_flight`. The assignment path reads that flag before setting held-run state. [`CurrentRun`](../../runner/src/daemon/supervisor.rs:1794) always contains a worker completion receiver, while cloud chat checks `current_run.is_some()` in the same file.

Failure scenario: inserting asynchronous disk persistence between the chat check and setting in-flight state lets a local chat begin in that interval. Alternatively, storing only a queue entry without `CurrentRun` allows cloud chat to see an idle runner. Retaining an artificial completion receiver can also clear a waiting reservation when its sender is dropped.

Correction: introduce a shared exclusive lane reservation and distinguish held assignment from optional executing worker. Reserve before storage, roll back on receipt failure, and derive cloud/local chat guards and heartbeat state from the reservation. Added to §§5, 6 and 9.

### R4 — P1: runner identity and timestamps are insufficient execution-attempt fences

Evidence: [`resolve_runner_for_run`](../../apps/api/pi_dash/runner/authentication.py:292) checks only run-to-runner ownership. [`_build_assign_msg`](../../apps/api/pi_dash/runner/services/matcher.py:492) carries neither an assignment-generation token nor `assigned_at`. Current lifecycle requests identify the run, but not its assignment generation. Session identity is available through [`OpenSessionResponse`](../../runner/src/cloud/http.rs:1154), separately from runtime bearer authorization.

Failure scenario: a stale queue record or delayed lifecycle message can refer to a run that has been requeued and assigned again to the same runner. An evicted daemon may still have valid runner credentials. A timestamp token would introduce formatting/precision assumptions without an existing wire contract.

Correction: mint a UUID assignment token in every assignment transaction; preserve it on redelivery; fence new-daemon capacity and lifecycle messages with it. Admission also validates current polling-session ownership. Keep legacy compatibility explicit, rather than implying that existing authentication supplies a session fence. Added to §7.

### R5 — P1: normal terminal delivery needs durability, not just shutdown failure replay

Evidence: [`drain_in_flight_runs`](../../runner/src/daemon/supervisor.rs:1262) journals shutdown failures, and [`drain_journal.rs`](../../runner/src/daemon/drain_journal.rs:1) stores `PendingRunFailure`. It is not a general completion/cancellation outbox. Normal worker sends are best effort and state clearing occurs directly in the worker.

Failure scenario: the machine frees a runner after completing or canceling queued work, loses the terminal POST and crashes. Two independent files for executable state and terminal retries can also leave contradictory recovery records. Releasing execution capacity and releasing cloud ownership are different transitions.

Correction: queue-managed attempts use one store for executable state and terminal-pending records, with exact payload, attempt token and stable idempotency key. Do not dual-write them into the legacy shutdown journal. The coordinator owns reservation clearing; permits may be freed after verified teardown while terminal delivery still holds the runner reservation. Added to §8.

### R6 — P1: worker completion is not sufficient proof of resource release

Evidence: [`wait_done`](../../runner/src/daemon/supervisor.rs:2582) treats a dropped oneshot receiver result as completion. Worker code uses `bridge.shutdown(...).await.ok()`. [`AgentBridge::shutdown`](../../runner/src/agent/mod.rs:610) delegates to individual drivers. Codex and Claude shutdown implementations wait through process-local exit watches, and child setup uses `kill_on_drop`; none of those inspected paths establishes a persisted, cross-restart process identity.

Failure scenario: releasing a permit on task error, dropped sender or unverified shutdown can admit replacement work while the prior agent remains alive. Hard daemon death cannot rely on Rust drop handlers. PID alone is unsafe for later cleanup because the OS can reuse it.

Correction: the permit remains owned until managed-agent exit is verified; preserve it in a cleanup/recovery state on uncertain teardown. Persist process identity and use appropriate containment/verification for restart recovery. Added to §§5 and 8. The selected managed-process guard and platform containment contract is now fixed in implementation.md §6; native verification remains an implementation test obligation, not an undecided design choice.

### R7 — P2: the initial mixed-cloud rollout discussion did not match current configuration

Evidence: [`DaemonConfig`](../../runner/src/config/schema.rs:337) contains one `cloud_url`. [`Supervisor`](../../runner/src/daemon/supervisor.rs:95) creates a shared transport from it, and runner activation rejects daemon identity changes. Multiple workspaces are not multiple cloud origins in this architecture.

Correction: remove mixed-cloud scheduling from v1. Negotiate capability for the daemon's configured cloud, carry it through welcome conversion, and specify behavior on reconnect or backend rollout inconsistency. Once managed entries exist, loss of capability must freeze admission rather than activate unbounded legacy behavior. Legacy WS is not an admission-capable transport merely because `RunnerOut` has a WS variant. Updated §11.

### R8 — P2: configuration changes must not restart active work or leave stale limits in hot-added runners

Evidence: [`spawn_save_config`](../../runner/src/tui/app.rs:823) uses `restart_and_verify`; [`RunnerSpawnCtx`](../../runner/src/daemon/supervisor.rs) retains daemon configuration for hot-add paths. `DaemonConfig` currently has no concurrency setting.

Correction: keep one live coordinator authoritative for the effective limit, use the dedicated IPC update, and ensure hot-added runners use that coordinator rather than a copied config-derived limit. Limit-only edits must bypass the generic restart path. This requirement was present in the draft and is retained; add a hot-add-after-limit-change test.

## Findings that support the proposed architecture

- Cloud `assigned` already occupies a runner. Local waiting does not require a new top-level AgentRun status or changing the matcher into a machine scheduler.
- [`StateHandle::set_current_run`](../../runner/src/daemon/state.rs:330) sets in-flight identity, derives busy status and resets per-run observability on ID change. These mechanisms can project a held waiting assignment, provided ownership lives in the coordinator.
- Current session-open redelivery intentionally includes assigned, never-started work while excluding running work. Durable local recovery can build on this distinction, with explicit reconciliation.
- The old `/queued/` endpoint really is retired and ignores new state changes. A new capacity contract is appropriate; reviving the worktree status would be misleading.
- Workspace setup precedes `Accept` and bridge startup, so a permit gate before `AssignWorker` naturally limits expensive preparation as well as execution.

## Closure and implementation validation obligations

The specification decisions are closed: SQLite owns receipt/outbox durability, the protocol uses typed decisions and assignment UUIDs, lock order is Runner → Session → Run, process scopes use a guard with durable stop receipts, and the product cap covers issue runs. The checks below remain required implementation evidence, not prerequisites for handing off the design.

1. Prove consistent session/runner/run locking and response semantics in both Python and Rust. Add tests for session eviction, admission retries and stale attempt events.
2. Implement and test the selected platform-specific managed-process containment/recovery contract. Fail closed on uncertain survivors; do not equate worker return with confirmed process exit.
3. Verify durable-store crash points, including receipt-before-ACK and terminal-state replacement, with injected failures. SQLite storage and its settings are specified; correctness still requires implementation tests.
4. Audit every assignment builder and new-daemon lifecycle sender for assignment tokens, not just the main pod drain.
5. Add transport and reservation race tests described above, plus hot-add after a live limit change.
6. Keep the product scope explicit: three **issue runs** by default, not three total agent processes. Chats on other runners and agent-launched subprocesses remain outside this cap. Counting chats requires another admission integration and should not be silently promised by this release.

Validation performed for this review: source inspection and document consistency/whitespace checks. No executable implementation was added, so no runtime tests were run.
