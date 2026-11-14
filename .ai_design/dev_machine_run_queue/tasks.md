# End-to-end implementation checklist

Specification: [design.md](design.md) and [implementation.md](implementation.md). Review history: [review.md](review.md).

All tasks are initially unchecked because this handoff changes documentation only. Implement in dependency order; do not equate a completed daemon queue with a completed feature. Update this file with actual validation evidence as work lands.

## T1 — Schema, capability and attempt identity

- [ ] Add Django migrations and Rust schema/entity parity for `assignment_token`, `assignment_protocol`, `capacity_wait`, and negotiated RunnerSession capability.
- [ ] Add defaulted capability fields to session open request, welcome response, HTTP welcome conversion and server protocol types.
- [ ] Centralize attempt construction; update pod drain, dedicated/direct assignment, requeue/reassignment and session redelivery in both backends.
- [ ] Implement legacy assigned-run adoption and preserve already-running legacy attempts without conversion.
- [ ] Prove fresh assignment rotates token, redelivery preserves it, and tokenless messages cannot mutate protocol-1 attempts.

Entry points: Python `runner/models.py`, `runner/migrations/`, `runner/services/matcher.py`, `runner/services/session_service.py`, `runner/views/sessions.py`; Rust `crates/db/src/entity/agent_run.rs`, runner session entities/migrations, `services/src/runner_sessions/live_drains.rs`, `drain.rs`, API runner session handlers; daemon `cloud/http.rs`, `cloud/protocol.rs`.

Done when both backends emit compatible protocol-1 assignments and old-daemon fixtures still pass. Add migration-from-old-schema and nullable-data coverage, not only clean-install tests.

## T2 — Typed admission, fencing and lifecycle

Depends on T1.

- [ ] Implement capacity waiting/admit/defer/reconcile contract and typed `RunnerCloudClient` methods.
- [ ] Apply Runner → Session → Run lock order to affected open/poll/admission/lifecycle paths; keep remote I/O outside transactions.
- [ ] Implement session eviction, attempt mismatch, replay revisions, stable idempotency and full duplicate-admit decisions.
- [ ] Add typed execution acceptance and terminal result handling; existing `RunnerOut::send` is not an admission oracle.
- [ ] Add held-token/capacity polling independently of optional agent observability.
- [ ] Update stall/recovery behavior without weakening offline expiry; serialize capacity presentation fields under existing workspace permissions.
- [ ] Run identical HTTP contract cases against Django and Rust, including retry after lost response and concurrent session replacement/cancel.

Entry points: Python `runner/views/run_endpoints.py`, `runner/authentication.py`, `runner/urls.py`, `runner/tasks.py`, serializers; Rust `api/src/runner_runs/run_endpoints.rs`, `api/src/runner_sessions/`, jobs `runner_runs/sweeps_chat.rs` and `offline_recovery.rs`; daemon `cloud/http.rs`.

Done when a terminal/canceled/stale/evicted response cannot launch work, and late waiting/terminal reports cannot alter a newer attempt.

## T3 — Durable queue and lane ownership

Depends on the T1/T2 wire types; implementation can use fake cloud responses until backend tests pass.

- [ ] Add SQLite dependency/lockfile, private paths and daemon lifetime lock.
- [ ] Implement serialized durable store, schema versioning, sequence allocation, tombstones and terminal outbox in proposed `daemon/run_queue_store.rs`.
- [ ] Implement FIFO coordinator, cancellation, admission credits, live limit changes and retry readiness in `daemon/run_queue.rs`.
- [ ] Separate held-assignment state from optional worker; implement atomic runner-lane reservation used by issue receipt, cloud chat and IPC chat.
- [ ] Construct one coordinator in supervisor and thread it through all startup/hot-add/reactivation paths.
- [ ] Add fault-injected commit/rollback/corruption tests and second-daemon lock test.

Done when fake workers demonstrate exact counts for five runners/limit two and ten runners/default three, with no duplicate ownership or bypass on hot-add.

## T4 — Durable transport receipt

Depends on T3.

- [ ] Split HTTP assignment dedupe into pending and committed receipt states.
- [ ] Implement receipt feedback with message ID and all stream IDs; pending duplicate must not inline-ACK.
- [ ] Route stream delivery and session-open redelivery through the same reservation/durable receipt path.
- [ ] Preserve nonassignment transport controls and reconnect behavior.
- [ ] Add delayed-disk-write plus duplicate-message tests, commit-failure retry and crash-after-commit-before-ACK tests.

Done when every ACKed assignment is either durably represented or authoritatively rejected, even if the daemon crashes at the next instruction.

## T5 — Managed process scope and verified teardown

Depends on T3; required before real execution is enabled.

- [ ] Add the hidden managed-process guard, private authenticated control channel, start barrier, scope records and stopped receipts.
- [ ] Implement Linux/macOS process-group anchor and Windows suspended-process/Job Object paths.
- [ ] Route issue workspace preparation, all supported agent drivers and their respawns through the managed scope while preserving protocol streams and environment behavior.
- [ ] Implement process identity checks and fail-closed unknown cleanup; never kill by PID alone or release on dropped oneshot.
- [ ] Test graceful exit, forced stop, agent crash, daemon SIGKILL/equivalent, guard crash, delayed exit, failed containment, and process-ID reuse using controlled fake processes.
- [ ] Add native macOS and Windows coverage to runner CI; the existing main runner test job is Linux-only and cannot prove these paths.

Done when a stopped receipt is trustworthy and no replacement worker starts while teardown is uncertain. No model/API credentials are required for these tests.

## T6 — Execution, terminal delivery and recovery

Depends on T2–T5.

- [ ] Gate `AssignWorker` before workspace setup; persist starting before launch and honor typed Accept before agent startup.
- [ ] Keep permit across retained approvals/reauth/backoff; route any new execution from resume/continuation through admission.
- [ ] Persist normal completion/failure/cancellation in finishing before teardown, then terminal_pending after verified exit; retry exact terminal payload/key.
- [ ] Remove direct managed reservation clearing from worker paths; coordinator owns it.
- [ ] Split shutdown/replay between managed SQLite outbox and legacy failure journal; never dual-journal one attempt.
- [ ] Restore reservations before session open, reconcile waiting order, never replay starting/executing, stop surviving scopes, and handle expired/canceled/deleted/reassigned runs.
- [ ] Integrate runner removal/reconfiguration, sign-out/revocation and identity-change refusal with queue ownership.
- [ ] Exercise crashes at every store/ACK/admit/start/stop/terminal-ACK boundary.

Done when normal restart preserves never-started FIFO work, interrupted executing work is finalized once, and cloud reconnect cannot resurrect terminal work.

## T7 — TUI, IPC and cloud presentation

Depends on T2/T3 status types; recovery action depends on T5/T6.

- [ ] Add shared `pidash-ipc` queue snapshot, limit-set/result and recovery request/result types with compatibility defaults.
- [ ] Add config validation/default and live persistence path; test a limit edit followed by hot-add, failed config write and concurrent config mutation.
- [ ] Add a machine-level TUI control available without runners; show counts, waiting positions, unsupported mode and storage/recovery diagnostics.
- [ ] Add Retry recovery, with no destructive reset/free-slot bypass.
- [ ] Update API-client contracts and both applicable web AgentRun detail consumers; distinguish local capacity wait from pinned cloud wait.
- [ ] Test stale reports, missing fields from old servers, workspace permission boundaries, queue cancellation and live decreases below active count.

Entry points: `pidash-ipc/src/protocol.rs`, `runner/src/ipc/server.rs`, TUI app/events/views, `packages/api-client/src/contracts/runners.ts`, `apps/web_new/src/features/issues/detail/waiting/`, existing web AgentRun services/detail UI.

Done when editing only the limit never restarts the daemon and UI reports actual effective mode rather than claiming the limit is always enforced.

## T8 — Compatibility, documentation and delivery

Depends on T1–T7.

- [ ] Verify old daemon/new cloud, new daemon/old cloud, rolling backend capability loss, active legacy attempts, and unknown local schema cases.
- [ ] Keep managed admission frozen on capability loss; no runtime fallback to unbounded starts.
- [ ] Document issue-only scope, default 3, live resizing, FIFO exceptions, capacity waiting, restart recovery, unsupported server behavior and drain-before-downgrade.
- [ ] Add the canonical end-to-end scenario below to the integration harness for both backends.
- [ ] Run relevant existing suites and new native platform tests; report any environmental test gap explicitly and leave the corresponding task unchecked.
- [ ] Final implementation report lists schema changes, test evidence, supported platforms and remaining limitations. Do not publish/deploy as part of this handoff unless separately requested.

### Canonical end-to-end scenario

With five runners on one daemon and limit two, hold one fake execution, deliver four additional runs through real HTTP session/poll transport, and observe one preparing/executing plus three durable local waits. Verify cloud `assigned` keeps each waiting runner busy. Finish a worker and observe the oldest ready wait start. Cancel a waiting run without launching its agent. Change the limit live to one and then three; verify drain and expansion. Restart the daemon while waiting and executing runs coexist, reconcile against the backend, and verify only never-started work survives as executable. Repeat with a lost ACK, duplicate delivery and a lost terminal response. Record process start/exit counters as well as cloud/UI state.

### Validation commands and environments

Follow the current repository test setup rather than inventing database credentials. From `runner/`, current CI uses:

```sh
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

Run the focused new tests while iterating, then these checks once the integrated change is ready. `rust-api/` has its own workspace; run its affected crate tests and required workspace checks. Use `rust-api/contract-tests/README.md` and `.github/workflows/pull-request-test-api.yml` to start Postgres/Redis and run the runner contracts against each backend. New capacity tests belong alongside existing runner contracts; no Django imports in the shared HTTP contract suite.

For changed new-web code, run `pnpm --filter web_new typecheck`, focused Vitest tests and the package's applicable lint/boundary checks. Follow existing API-client and legacy-web scripts for their changes. Run native process tests on Linux, macOS and Windows; cross-compilation alone is not runtime evidence.

## Definition of done

- [ ] T1–T8 complete, including both backends and all three supported OS families.
- [ ] Every acceptance scenario in design.md has a named automated test or documented integration result.
- [ ] No code path starts a managed issue worker without a coordinator permit and current cloud authorization.
- [ ] Every acknowledged assignment survives local restart or has an authoritative nonexecution disposition.
- [ ] No uncertain process exit or stale attempt can silently release capacity or mutate replacement work.
- [ ] No unresolved TODO changes queue ownership, cancellation, compatibility, durability or process recovery semantics.
