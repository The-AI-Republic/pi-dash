# TRACE — D-14 runner:sessions fixtures (PIDASHCONV-546)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from (drift baseline): `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(no drift on this checkout — `git diff Ported-from..rust-dev` empty for all D-14 files;
line counts re-verified: sessions.py 610, machine_sessions.py 352,
session_service.py 508, outbox.py 737, machine_outbox.py 393, pubsub.py 176,
matcher.py 465).
Gate: PIDASHCONV-23 (`rust-api/contract-tests/runner/test_daemon_sessions.py`
+ `test_ws_close.py`); key sets cross-checked against the open/poll/delete
shape assertions there (`long_poll_interval_secs == 25`, `protocol_version == 4`,
`{"detail": "runner_id_mismatch"}` 401 on cross-runner open — lowercase
`detail` everywhere: DRF dispatch renders `{"detail": ...}` and both poll
views spell it lowercase too; hexdump-verified in source and probe bytes).

Method (all fixtures): every value was produced by running the named Python
code — the s4 Django interpreter (Django 4.2.30) against scratch Postgres
`pidash_546` (full migrated schema) and real Redis DB 14 behind a logging
proxy (command traces) — except where a fixture's `_method` says otherwise.
D-13 auth is stubbed at the view boundary (`request.auth_runner` /
`request.auth_machine_token` set directly) except for the 401 cases, which
run the real auth classes + the real DRF dispatch / exception handler.
Probe scripts live outside the repo (`/tmp/pidash546/`, kept for re-runs).

## FX-RSES-01 models

- `fx-rses-01-models.json` — `runner/models.py` `RunnerSession:688-720`
  (fields `:699-705`, `Meta` `:707-720`) and `MachineSession:723-762`
  (fields `:739-747`, `Meta` `:749-762`); FK-contract column lists only
  (owned by D-13/D-15, no fixture ownership) for `Runner:382-498`,
  `DevMachine:331-376`, `AgentRun:872-1075`, `Pod:52-122`,
  `RunnerLiveState:1454-1512`, `AgentRunEvent:1162-1175`.

## FX-RSES-02 shapes

- `fx-rses-02-shapes.json`
  - Runner welcome + 201: `runner/views/sessions.py:266-285`
    (conditional version keys `:273-276`); machine welcome + 201:
    `runner/views/machine_sessions.py:121-131`.
  - Poll 200 envelopes: `sessions.py:595-601`, `machine_sessions.py:339-345`.
  - 426: `sessions.py:97-119`; 403 runner: `:136-141`, `:296-301`, `:567-568`;
    403 machine: `machine_sessions.py:54-66`, `:77-82`, `:141-147`, `:300-307`.
  - 409 project: `sessions.py:147-154`; 409 evicted: `:338-352`, `:590-591`,
    machine `:180-194`, `:332-335`.
  - 503: `sessions.py:202-213` (+ bookkeeping `:399-412`);
    `SET LOCAL` values: `_bound_txn_waits:50-71` (+ machine-open asymmetry:
    no `_bound_txn_waits` call at `machine_sessions.py:88-108`).
  - 400 / 401 / 405 poll: `sessions.py:560-576`, machine `:289-319`
    (lowercase `detail`, like dispatch); DRF-dispatch 401/405 on the
    open/delete endpoints via
    `pi_dash/authentication/adapter/exception.py:18-36` + DRF default handler
    (`{"detail": ...}`, pinned by contract test); auth-level 401
    `runner_id_mismatch` (resolver_match URL check,
    `runner/authentication.py:124-126`, setup-minted JWT) vs view-level 403
    (stubbed auth); machine `mt_`-bogus 401 `machine_token_invalid` vs
    non-`mt_` Bearer [REDACTED] 403 `dev_machine_mismatch` (auth returns None,
    `authentication.py:218-220`); two-poll delivery (PEL `0` replay misses
    pre-queued entries, follow-up `>` delivers).
  - 204 deletes: `sessions.py:302-312`, `machine_sessions.py:148-159`.
  - `permission_classes=[]` / `throttle_classes=[]` on all four endpoints
    (`sessions.py:129-130`, `:292-293`; `machine_sessions.py:73-74`,
    `:138-139`) — verified, no permissions sub-issue.
  - Long-poll slice invariants: `sessions.py:462-538` (`BLOCK 0` guard
    `:522-523`, eviction break `:535-536`, `mark_pel_drained` after
    `use_zero` read `:592-593`); machine twins
    `machine_sessions.py:211-272`, `:336-337`.

## FX-RSES-03 envelopes

- `fx-rses-03-envelopes.json`
  - `_VALID_TYPES` / `_OFFLINE_REJECT`: `runner/services/outbox.py:38-67`,
    `runner/services/machine_outbox.py:49-65`.
  - `_serialize`: `outbox.py:120-136`; `_decode_read_result`: `:164-189`
    (shared by the machine outbox via `:37-40` re-export).
  - `_ensure_envelope`: `runner/services/pubsub.py:39-42`;
    `runner_group` legacy name `:34-36`.
  - `_build_assign_msg`: `runner/services/matcher.py:306-330`.
  - `resume_ack` vs `cancel` variants:
    `runner/services/session_service.py:473-508`; cancel-first redeliver
    `:400-451`.
  - `revoke` / `remove_runner` frames + `send_connection_revoke` alias:
    `pubsub.py:110-176`; eviction pubsub body: `outbox.py:544-556`,
    machine `:373-385`.
  - `close_runner_session` row revoke + marker + eviction signal:
    `pubsub.py:81-108`.
  - Close-code-1008 semantics kept from the skip stub
    (`runner/consumers.py:29-42`, Dead Python Code §4, NOT ported):
    `ws/runner/` never yields a channel — pinned by `test_ws_close.py`
    (uvicorn renders close-before-accept as handshake 403 / unknown-path
    500; runserver renders 404/404).

## FX-RSES-04 keys

- `fx-rses-04-keys.json` — key builders `outbox.py:89-115`,
  `machine_outbox.py:84-106` + `command_result_key:339-340`;
  stream-id utils `outbox.py:687-737`.

## FX-RSES-05 outbox ops

- `fx-rses-05-outbox-ops.json`
  - `enqueue_for_runner` / `enqueue_for_machine`:
    `outbox.py:220-253`, `machine_outbox.py:161-193` (live vs
    offline-buffer vs `RunnerOfflineError` / `MachineOfflineError` vs
    redis-None; `OFFLINE_STREAM_MAXLEN` / `OFFLINE_STREAM_TTL_SECS`
    application; `ValueError` on unknown type).
  - `drain_offline_into_live`: `outbox.py:256-283`, machine `:196-222`.
  - `claim_pending_for_new_session` (+ `delete_consumer`):
    `outbox.py:286-340` + `:343-369` — spins forever on non-empty PEL
    (JUSTID cursor bug; see ported-bug note 3), returns 0 on empty PEL
    without deleting the old consumer; `delete_consumer` direct vectors
    (falsy → 0 with no Redis call).
  - `read_for_session` / `aread_for_session`: `outbox.py:452-523`,
    machine `:247-308`; `ack_for_session`: `outbox.py:526-538`,
    machine `:311-323`.
  - PEL markers: `outbox.py:428-449` (TTL = 2×`ACCESS_TOKEN_TTL_SECS`),
    machine `:225-244`.
  - Eviction publish: `outbox.py:544-556`, machine `:373-385`.
  - Cleanup zset + stream delete: `outbox.py:562-606`, machine `:388-393`.
  - `reap_idle_consumers`: `outbox.py:382-425` (default idle floor).
  - `safe_trim_runner_stream`: `outbox.py:609-681` (floor =
    `min(time_cutoff, min_pending - 1 | last_delivered)`; redis-py sends
    `MINID ~` — `approximate=True` default — so single-node streams trim 0
    and bulk trims whole nodes only: 300-entry case keeps 2200-0+ against
    floor 2289-0).
  - `active_session_id_for_runner` / `_for_machine`: `outbox.py:203-214`,
    machine `:144-155`.
  - Command results: `machine_outbox.py:336-367` (TTL 900, corrupt→None).
  - No Celery tasks in D-14 files (grep-verified zero task decorators in
    all 7 sources); the `tasks.py` stream-cleanup loop is D-15's and calls
    the `safe_trim` / `due_runners` / `remove_marker` / `delete_stream`
    helpers recorded here.

## FX-RSES-06 session service

- `fx-rses-06-session-service.json`
  - `apply_hello` (+ `_merge_dev_metadata`, `_agent_capabilities`):
    `runner/services/session_service.py:58-147`.
  - `reap_stale_busy_runs`: `:150-290` (heartbeat-ts clamp `:172-179`,
    `effective_cutoff` min() `:189-195`, `exclude_redeliverable` set
    difference `:197-204`, cancel→CANCELLED barrier `:243-254`,
    `finalize_agent_run` FAILED payload `heartbeat_reaped` `:260-272`
    — finalizer owned by D-15, payload recorded here — on_commit drain
    scheduling `:277-290`).
  - `upsert_runner_live_state`: `:336-397` (no-op cases, wipe on
    `observed_run_id` change `:373-381`, no-overwrite-on-missing,
    `model`→`llm_model` 128-truncation `:391-394`); `normalize_usage`
    vectors from `runner/services/usage.py:129-148` (+ `coerce_token`
    `:69-79`, provider/counter pickers `:82-126`).
  - `mark_runner_online` / `mark_runner_offline` (REVOKED exclusion):
    `:454-459`; `resolve_runner_project_slug`: `:462-470`;
    `build_resume_ack` last_seq branch covered under FX-RSES-03.

## FX-RSES-07 matcher

- `fx-rses-07-matcher.json`
  - `select_runner_in_pod`: `runner/services/matcher.py:89-120`
    (SKIP LOCKED, ONLINE + heartbeat-fresh, DESKTOP_BUNDLED exclusion,
    busy exclusion, `-last_heartbeat_at` order).
  - `next_queued_run_for_pod`: `:123-144` (unpinned, MACHINE_EXECUTORS,
    oldest).
  - `next_for_runner`: `:147-191` (pin-rank CASE ordering, provisioning
    executor split, `filter_runs_usable_by_runner` passthrough note —
    filter owned by D-11/D-13, applied here).
  - `drain_pod` / `drain_for_runner` (+ `_by_id` wrappers): `:194-303`
    (assignment writes + on_commit `send_to_runner` dispatch).
  - Legacy `select_runner_for_run`: `:338-360`.
  - `can_register_another` / `count_active`: `:363-372`, `:451-460`
    (cap + REVOKED/DESKTOP exclusions).
  - `pod_has_runner_for_issue_principal`: `:375-448` (managed-runner
    branch, assignee-via-through-table branch, REVOKED exclusion).
  - `eligible_for_assignment`: `:463-465`; consts `:44-81`
    (`HEARTBEAT_GRACE`, `NON_TERMINAL_STATUSES`, `BUSY_STATUSES`,
    `MACHINE_EXECUTORS` from `core/agent_execution.py:23`).

## Port-existing-bug notes (for the layer PRs)

Bugs observed while recording (translate, don't redesign — the port
reproduces them; each layer PR lists the ones it ports):

1. Django async-Redis flake: the poll views share one module-global async
   Redis client across requests; under the dev server each request runs on
   a fresh event loop, so a poll can answer 500 `RuntimeError: Event loop
   is closed` instead of 200. The contract suite retries polls on 500
   (`test_daemon_sessions.py:31-47`); the Rust port must return the 200
   envelope (the retry proves Django's 500 is the bug).
2. `RunnerOfflineError` from drain dispatch is unhandled on the
   reaper/session-open drain path (`session_service.py:277-290`): assigning
   to an ONLINE-but-sessionless runner persists the ASSIGNED row, then the
   `on_commit` `send_to_runner` raises out of the commit — a 500 for the
   triggering poll/open. (The poll-ready drain at `sessions.py:447-456`
   *is* wrapped; the reaper one is not.) Recorded in FX-RSES-07
   (`drain_for_runner_sessionless`).
3. `claim_pending_for_new_session` never terminates on a non-empty PEL
   (`outbox.py:286-340`): called with `justid=True`, redis-py 5.0.4's
   `parse_xautoclaim` returns `response[1]` (the flat id list),
   discarding the cursor — the loop mistakes `result[0]` (a stream id)
   for the cursor and re-issues XAUTOCLAIM from the same id forever,
   accumulating `len()` of id strings as the count. Production pins
   `redis==5.0.4`, so this is live; the session-open wrapper
   (`sessions.py:225-232`) catches only `(RedisError, OSError)`, never
   a hang — an open with pending PEL entries wedges the HTTP worker.
   The `if old_consumer: delete_consumer` branch is unreachable in
   practice (only an empty PEL returns, and it returns before the
   delete). Recorded in FX-RSES-05 (`claim_justid_shape`,
   `claim_pending_nonempty_spins`, `claim_pending_none_consumer`,
   `claim_pending_empty_pel` — spinner stopped by flushing, which the
   loop catches as NOGROUP).
4. `reap_idle_consumers` returns the sum of removed consumers' *pending
   counts*, not the number of consumers removed (`outbox.py:421`: `removed
   += delete_consumer(...)`, and `XGROUP DELCONSUMER` reports pending
   entries) — reaping one zero-pending consumer returns 0 even though
   the consumer is gone. Recorded in FX-RSES-05 (`reap_idle_consumers`:
   `reaped: 0` with `consumers_after` proving the deletion).
