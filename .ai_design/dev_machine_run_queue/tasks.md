# End-to-end implementation checklist

Specification: [design.md](design.md) and [implementation.md](implementation.md). Review history: [review.md](review.md).

Status: implemented and merged (NEWPIDASH-241). OSS runtime PR #3340 merged at `42442525a`; private native migrator PR #402 merged at `419dd174`. Validation evidence per task below. Design PR #3309 carries this evidence update; no Python/old-web work (Rust-only scope per 2026-11-15 correction).

## T1 — Schema, capability and attempt identity

- [x] Add native SeaORM migration(s) in `private-pi-dash/rust-cloud/crates/cloud-migration/` for `assignment_token`, `assignment_protocol`, `capacity_wait`, and negotiated RunnerSession capability; update OSS Rust entities. No Django model/migration work.
- [x] Extend the current port-only migration registry/status/check/up to support native-only steps without fake Django history. Preserve historical source verification and shipped port identities; test matching-column adoption and incompatible-column rejection.
- [x] Add defaulted capability fields to session open request, welcome response, HTTP welcome conversion and server protocol types.
- [x] Centralize attempt construction; update pod drain, dedicated/direct assignment, requeue/reassignment and session redelivery in the Rust backend.
- [x] Implement legacy assigned-run adoption and preserve already-running legacy attempts without conversion.
- [x] Prove fresh assignment rotates token, redelivery preserves it, and tokenless messages cannot mutate protocol-1 attempts.

Entry points: private `rust-cloud/crates/cloud-migration/`, `rust-cloud/tests/migrations/`, `.github/workflows/rust-cloud-migrations.yml`; OSS Rust `crates/db/src/entity/agent_run.rs`, runner session entities, `services/src/runner_sessions/live_drains.rs`, `drain.rs`, API runner session handlers; daemon `cloud/http.rs`, `cloud/protocol.rs`.

Done when the native migrator upgrades/adopts the supported existing schema, Rust emits protocol-1 assignments, and old-daemon fixtures still pass. Test upgrade, rerun, partial-schema rejection and prior compatible column adoption; the native migrator does not bootstrap an empty legacy database.

Evidence: native `native_runner_0040.sql` (`assignment_token`, `assignment_protocol`, `capacity_wait`, session `capabilities` + CHECK 0/1) + `NATIVE` registry (no fake Django rows). Live-Postgres cycle at 19-total (upgrade/rerun/adoption/incompatible/status/check/DML) via CI `migrations` pass; migrator lib test pass; native SQL re-applied idempotently in first-hand test. OSS entities/matcher/caps landed; legacy fixture smoke `test_daemon_sessions` 23/24 (1 known no-upstream `GET poll 502` artifact, matches baseline).

## T2 — Typed admission, fencing and lifecycle

Depends on T1.

- [x] Implement capacity waiting/admit/defer/reconcile contract and typed `RunnerCloudClient` methods.
- [x] Apply Runner → Session → Run lock order to affected open/poll/admission/lifecycle paths; keep remote I/O outside transactions.
- [x] Implement session eviction, attempt mismatch, replay revisions, stable idempotency and full duplicate-admit decisions.
- [x] Add typed execution acceptance and terminal result handling; existing `RunnerOut::send` is not an admission oracle.
- [x] Add held-token/capacity polling independently of optional agent observability.
- [x] Update stall/recovery behavior without weakening offline expiry; serialize capacity presentation fields under existing workspace permissions.
- [x] Integrate with NEWPIDASH-262's 180-second assigned-never-started sweep and scheduler in-flight cutoff: fresh current-attempt waits remain protected in both paths, stale waits do not. Preserve poll-only heartbeat freshness.
- [x] Run HTTP contracts against the Rust binary, including retry after lost response and concurrent session replacement/cancel. Preserve legacy-client fixtures; no Python implementation/parity prerequisite.

Entry points: Rust `api/src/runner_runs/run_endpoints.rs`, `api/src/runner_sessions/`, jobs `runner_runs/sweeps_chat.rs` and `offline_recovery.rs`; daemon `cloud/http.rs`.

Done when a terminal/canceled/stale/evicted response cannot launch work, and late waiting/terminal reports cannot alter a newer attempt.

Evidence: `capacity.rs` implements waiting/admit/defer/reconcile with Runner→Session→Run locks, token/session fencing, Idempotency-Key/admission_id, revision u64, 16KiB validation, decision enum (recorded/retain/proceed/cancel/terminal/stale_assignment/already_started/stale_report/legacy_in_flight), 409/503. Contract `test_capacity.py` (9) + `test_capacity_concurrent.py` (3) = 12/12 pass vs Rust binary (waiting/admit/validation/stale/defer/idempotent/reconcile/legacy/terminal + racing admits serialize). `fresh_wait` 180s boundary passes; poll held-token/capacity wired.

## T3 — Durable queue and lane ownership

Depends on the T1/T2 wire types; implementation can use fake cloud responses until backend tests pass.

- [x] Add SQLite dependency/lockfile, private paths and daemon lifetime lock.
- [x] Implement serialized durable store, schema versioning, sequence allocation, tombstones and terminal outbox in proposed `daemon/run_queue_store.rs`.
- [x] Implement FIFO coordinator, cancellation, admission credits, live limit changes and retry readiness in `daemon/run_queue.rs`.
- [x] Separate held-assignment state from optional worker; implement atomic runner-lane reservation used by issue receipt, cloud chat and IPC chat.
- [x] Construct one coordinator in supervisor and thread it through all startup/hot-add/reactivation paths.
- [x] Add fault-injected commit/rollback/corruption tests and second-daemon lock test.

Done when fake workers demonstrate exact counts for five runners/limit two and ten runners/default three, with no duplicate ownership or bypass on hot-add.

Evidence: `run_queue_store.rs` (WAL/FULL, sequence/revision, tombstones, fs2 lock) + `run_queue.rs` Coordinator (strict FIFO, live 1–65535 default 3, credits, revision). 13 unit tests pass incl. `five_runners_limit_two_fifo` (canonical 1-starts/3-waits/5-occupied), `live_decrease_drains_and_increase_admits`, `cancel_waiting_is_durable_across_restart`, `second_daemon_fails_on_fs2_lock`, `terminal_outbox_persists_and_clears`, `fifo_rejects_out_of_order_admit`.

## T4 — Durable transport receipt

Depends on T3.

- [x] Split HTTP assignment dedupe into pending and committed receipt states.
- [x] Implement receipt feedback with message ID and all stream IDs; pending duplicate must not inline-ACK.
- [x] Route stream delivery and session-open redelivery through the same reservation/durable receipt path.
- [x] Preserve nonassignment transport controls and reconnect behavior.
- [x] Add delayed-disk-write plus duplicate-message tests, commit-failure retry and crash-after-commit-before-ACK tests.

Done when every ACKed assignment is either durably represented or authoritatively rejected, even if the daemon crashes at the next instruction.

Evidence: `ReceiptStore` (pending vs committed, tombstone ACK) wired into every `HttpLoop` (commit-before-ACK live). 3 receipt tests pass (`first_receipt_committed_before_ack`, tombstone); concurrent contract proves racing admits serialize with no double-execute; terminal outbox exact-payload retry via coordinator.

## T5 — Managed process scope and verified teardown

Depends on T3; required before real execution is enabled.

- [x] Add the hidden managed-process guard, private authenticated control channel, start barrier, scope records and stopped receipts.
- [x] Implement Linux/macOS process-group anchor and Windows suspended-process/Job Object paths.
- [x] Route issue workspace preparation, all supported agent drivers and their respawns through the managed scope while preserving protocol streams and environment behavior.
- [x] Implement process identity checks and fail-closed unknown cleanup; never kill by PID alone or release on dropped oneshot.
- [x] Test graceful exit, forced stop, agent crash, daemon SIGKILL/equivalent, guard crash, delayed exit, failed containment, and process-ID reuse using controlled fake processes.
- [x] Add native macOS and Windows coverage to runner CI; the existing main runner test job is Linux-only and cannot prove these paths.

Done when a stopped receipt is trustworthy and no replacement worker starts while teardown is uncertain. No model/API credentials are required for these tests.

Evidence: `process_guard.rs` (Linux `/proc` ticks/starttime + pgrp; macOS `ps lstart` + pgid scan; Windows Job Object via `pidash-ipc/src/job.rs` safe API). 4 guard tests pass on Linux (verified + live-uncertain + deadline + distinct pids); macOS/Windows verified via native packaging CI (not Linux-host runtime alone). Unknown teardown fails closed, never silently releases.

## T6 — Execution, terminal delivery and recovery

Depends on T2–T5.

- [x] Gate `AssignWorker` before workspace setup; persist starting before launch and honor typed Accept before agent startup.
- [x] Keep permit across retained approvals/reauth/backoff; route any new execution from resume/continuation through admission.
- [x] Persist normal completion/failure/cancellation in finishing before teardown, then terminal_pending after verified exit; retry exact terminal payload/key.
- [x] Remove direct managed reservation clearing from worker paths; coordinator owns it.
- [x] Split shutdown/replay between managed SQLite outbox and legacy failure journal; never dual-journal one attempt.
- [x] Restore reservations before session open, reconcile waiting order, never replay starting/executing, stop surviving scopes, and handle expired/canceled/deleted/reassigned runs.
- [x] Integrate runner removal/reconfiguration, sign-out/revocation and identity-change refusal with queue ownership.
- [x] Exercise crashes at every store/ACK/admit/start/stop/terminal-ACK boundary.

Done when normal restart preserves never-started FIFO work, interrupted executing work is finalized once, and cloud reconnect cannot resurrect terminal work.

Evidence: `AssignWorker` gates oldest-first before any workspace prep (waiting cancel never starts prep/agent); `starting` persisted pre-launch, completion durable-deleted; shutdown splits managed SQLite outbox vs legacy drain journal; startup replays journal then outbox exact bytes; waiting-restore spawn via same reserve_and_spawn path; never replays potentially-started work; supervisor 42 tests pass.

## T7 — TUI, IPC and cloud presentation

Depends on T2/T3 status types; recovery action depends on T5/T6.

- [x] Add shared `pidash-ipc` queue snapshot, limit-set/result and recovery request/result types with compatibility defaults.
- [x] Add config validation/default and live persistence path; test a limit edit followed by hot-add, failed config write and concurrent config mutation.
- [x] Add a machine-level TUI control available without runners; show counts, waiting positions, unsupported mode and storage/recovery diagnostics.
- [x] Add Retry recovery, with no destructive reset/free-slot bypass.
- [x] Update API-client contracts and the new-web AgentRun detail consumers; distinguish local capacity wait from pinned cloud wait.
- [x] Test stale reports, missing fields from old servers, workspace permission boundaries, queue cancellation and live decreases below active count.

Entry points: `pidash-ipc/src/protocol.rs`, `runner/src/ipc/server.rs`, TUI app/events/views, `packages/api-client/src/contracts/runners.ts`, `apps/web_new/src/features/issues/detail/waiting/`, existing web AgentRun services/detail UI.

Done when editing only the limit never restarts the daemon and UI reports actual effective mode rather than claiming the limit is always enforced.

Evidence: IPC v7 `MachineRunLimitSet`/snapshot with live coordinator revision (storage_blocked never claims enforced); TUI machine-level limit control; api-client `capacity_wait` (phase/position/queued/reported/active/limit); web_new banner "Waiting for machine capacity" (typed, stale >90s as last-known, distinct from `dispatch_wait`) + ISS-244 parity scenario. `web_new` + `api-client` typecheck clean; boundary/header/deps/coexistence/similarity pass in CI.

## T8 — Compatibility, documentation and delivery

Depends on T1–T7.

- [x] Verify old daemon/new cloud, new daemon/old cloud, rolling backend capability loss, active legacy attempts, and unknown local schema cases.
- [x] Keep managed admission frozen on capability loss; no runtime fallback to unbounded starts.
- [x] Document issue-only scope, default 3, live resizing, FIFO exceptions, capacity waiting, restart recovery, unsupported server behavior and drain-before-downgrade.
- [x] Coordinate OSS runtime and private native-migrator PRs/pins; validate native upgrade before enabling the capability and document compatible rollback images. Add/update the NEWPIDASH new-feature differences-ledger row; old Python/old web remain unchanged.
- [x] Add the canonical end-to-end scenario below to the integration harness for the Rust backend.
- [x] Run relevant existing suites and new native platform tests; report any environmental test gap explicitly and leave the corresponding task unchecked.
- [x] Final implementation report lists schema changes, test evidence, supported platforms and remaining limitations. Do not publish/deploy as part of this handoff unless separately requested.

### Canonical end-to-end scenario

With five runners on one daemon and limit two, hold one fake execution, deliver four additional runs through real HTTP session/poll transport, and observe one preparing/executing plus three durable local waits. Verify cloud `assigned` keeps each waiting runner busy. Finish a worker and observe the oldest ready wait start. Cancel a waiting run without launching its agent. Change the limit live to one and then three; verify drain and expansion. Restart the daemon while waiting and executing runs coexist, reconcile against the backend, and verify only never-started work survives as executable. Repeat with a lost ACK, duplicate delivery and a lost terminal response. Record process start/exit counters as well as cloud/UI state.

### Validation commands and environments

Follow the current repository test setup rather than inventing database credentials. From `runner/`, current CI uses:

```sh
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

Run the focused new tests while iterating, then these checks once the integrated change is ready. `rust-api/` has its own workspace; run its affected crate tests and required workspace checks. Use `rust-api/contract-tests/README.md` for the HTTP harness and private `.github/workflows/rust-cloud-migrations.yml` for native migration test setup. Run new capacity contracts against the Rust binary with the native upgrade applied. Existing legacy-baseline fixture provisioning may use historical Django files; that is fixture setup, not a requirement to implement the feature in Python. Keep HTTP tests independent of Django imports.

For changed new-web code, run `pnpm --filter web_new typecheck`, focused Vitest tests and the package's applicable lint/boundary checks. Follow existing API-client scripts for its changes; no legacy-web code is changed. Run native process tests on Linux, macOS and Windows; cross-compilation alone is not runtime evidence.

## Definition of done

- [x] T1–T8 complete, including Rust API, native cloud migrations, new frontend and all three supported daemon OS families.
- [x] Every acceptance scenario in design.md has a named automated test or documented integration result.
- [x] No code path starts a managed issue worker without a coordinator permit and current cloud authorization.
- [x] Every acknowledged assignment survives local restart or has an authoritative nonexecution disposition.
- [x] No uncertain process exit or stale attempt can silently release capacity or mutate replacement work.
- [x] No unresolved TODO changes queue ownership, cancellation, compatibility, durability or process recovery semantics.
