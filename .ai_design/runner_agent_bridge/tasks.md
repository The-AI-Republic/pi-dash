# Runner ↔ AI Agent Communication: Observability and Health Bridge — Tasks

Purpose: track the implementation of the per-active-run
observability snapshot, the bridge-owned process-exit notification,
and the cloud-side stall watchdog described in `design.md`.

Companion docs in this directory:

- `design.md` — architecture, wire shape, ingestion rules, decisions

How to use this file:

- Keep task status in-place with checkboxes.
- Add PR links or issue ids inline after the task text.
- Do not delete completed tasks; strike or annotate only if scope
  changes.
- Section refs like `design.md §4.5.2` point at the normative spec;
  treat them as the contract.
- Phase A and Phase B can ship in either order because the runner
  fields are gated by `agent_observability_v1` and the cloud
  ingestion is a no-op when fields are absent. **Phase B must
  ship before runners enable the flag in production**, so cloud
  rolls first by convention.

## Milestones

- [ ] Phase A: runner — bridge process-exit notification, runner-
      side state plumbing, `PollStatus` extension, opt-in token /
      turn parsing, all gated behind `agent_observability_v1`
- [ ] Phase B: cloud — `RunnerLiveState` table, poll-handler
      ingestion, server-side stall watchdog with explicit run-id
      match
- [ ] Phase C: operator surface — web UI panel + optional
      observability JSON endpoint
- [ ] Roll out: enable `agent_observability_v1=true` per fleet,
      monitor watchdog firing rate vs. runner-internal stall count

## 1. Phase A — Runner side

### 1.1 Bridge process handle (`design.md` §4.4)

- [ ] Define `AgentProcessHandle` in `runner/src/agent/mod.rs`:
  - [ ] `pub pid: Option<u32>`
  - [ ] `pub exit_rx: tokio::sync::watch::Receiver<Option<ExitSnapshot>>`
- [ ] Define `ExitSnapshot` in the same file:
  - [ ] `pub status_code: Option<i32>`
  - [ ] `pub signal: Option<i32>`
  - [ ] `pub observed_at: chrono::DateTime<Utc>`
- [ ] Add `AgentBridge::process_handle(&self) -> AgentProcessHandle`
      enum-dispatched to each concrete bridge.

### 1.2 Codex AppServer refactor — `Child` ownership (`design.md` §4.4 implementation note)

- [ ] In `runner/src/codex/app_server.rs`, replace `child: Child`
      with `kill_tx: mpsc::Sender<()>` (or oneshot if kill fires
      exactly once) and store the new `exit_rx`.
- [ ] Spawn a wait task immediately after `cmd.spawn()`:
  - [ ] `tokio::spawn` that owns `Child` exclusively.
  - [ ] `select!` on `kill_rx.recv()` (→ `child.start_kill()`) and
        `child.wait()` (→ build `ExitSnapshot`, send via
        `exit_tx`, exit task).
  - [ ] Capture `child.id()` before moving `Child` into the task —
        this becomes `AgentProcessHandle.pid`.
- [ ] Rewrite `AppServer::shutdown(grace)`:
  - [ ] Send a `KillRequest` via `kill_tx`.
  - [ ] Await `exit_rx.changed()` with a `tokio::time::timeout`.
  - [ ] Removes the existing direct calls to `self.child.wait()` /
        `self.child.start_kill()` at lines 60-72.
- [ ] Add a unit test that exercises `spawn → kill → exit_rx
    observed → ExitSnapshot.status_code is Some(<signal-derived>)`.

### 1.3 Claude process refactor — same shape

- [ ] Repeat 1.2 for `runner/src/claude_code/process.rs`. Same
      `Child`-ownership flip, same `kill_tx` / `exit_rx` exposure.
- [ ] Verify `claude_code::Bridge` exposes `process_handle()`.

### 1.4 `StateHandle` snapshot fields (`design.md` §4.3)

- [ ] In `runner/src/daemon/state.rs::Inner`, add:
  - [ ] `last_event_at: Mutex<Option<DateTime<Utc>>>`
  - [ ] `last_event_kind: Mutex<Option<String>>`
  - [ ] `last_event_summary: Mutex<Option<String>>`
  - [ ] `agent_pid: Mutex<Option<u32>>`
  - [ ] `agent_subprocess_alive: Mutex<Option<bool>>`
  - [ ] `tokens: Mutex<Option<TokenUsage>>`
  - [ ] `turn_count: Mutex<Option<u32>>`
- [ ] Define `TokenUsage { input, output, total: u64 }` in a new
      module `runner/src/daemon/observability.rs`.
- [ ] Add `StateHandle` async helpers:
  - [ ] `note_agent_event(ts, kind, summary)`
  - [ ] `set_agent_pid(Option<u32>)`
  - [ ] `set_agent_alive(bool)`
  - [ ] `set_tokens(TokenUsage)`
  - [ ] `incr_turn()`
  - [ ] `reset_run_snapshot()` (wipes all of the above to None)
- [ ] Modify `set_current_run` per the §4.3 pseudocode: call
      `reset_run_snapshot()` when `next.run_id != prev.run_id` AND
      when `prev.is_none() && next.is_some()`. Do **not** call
      it when the same run id is re-stamped.
- [ ] Unit tests for `StateHandle`:
  - [ ] `set_current_run(Some(rid))` twice with same rid → snapshot
        is **not** reset.
  - [ ] `set_current_run(Some(rid_a))` then `set_current_run(Some(rid_b))`
        → snapshot is reset.
  - [ ] `set_current_run(None)` → snapshot fields preserved
        (last poll still carries the terminal values).
  - [ ] `note_agent_event` updates `last_event_at`, `last_event_kind`,
        `last_event_summary` atomically.

### 1.5 Supervisor wiring (`design.md` §4.3 call sites)

- [ ] In `runner/src/daemon/supervisor.rs::pump_events`, before the
      existing `match event` body:
  - [ ] Compute `kind = kind_of(&event)` and `summary = summary_of(&event)`.
  - [ ] Call `state.note_agent_event(Utc::now(), kind, summary).await`.
- [ ] After `AgentBridge::spawn_*` returns Ok in the run loop:
  - [ ] Read `bridge.process_handle()`.
  - [ ] Call `state.set_agent_pid(handle.pid).await`.
  - [ ] Call `state.set_agent_alive(true).await`.
  - [ ] `tokio::spawn` a small task that watches `handle.exit_rx`
        and calls `state.set_agent_alive(false).await` when the
        receiver yields `Some(ExitSnapshot)`.
- [ ] `kind_of(&BridgeEvent)` formatter helper, in
      `runner/src/daemon/observability.rs`. One match arm per
      variant. For `Raw { method, .. }` use `method` directly.
      Result is length-capped at 64 chars.
- [ ] `summary_of(&BridgeEvent)` formatter helper, same module.
      Structure-only output: never includes prompt, model output,
      or file content. For each event variant, surface
      method-name + identifiers + duration (where available).
      Length-capped at 200 chars. Add unit tests asserting that
      a tool/exec event's args do **not** appear in the summary.

### 1.6 Optional, opt-in: token / turn extraction (`design.md` §4.3)

- [ ] Behind a sub-flag of `agent_observability_v1`
      (`agent_observability_v1.parse_codex_token_count`,
      default true on Codex):
  - [ ] In the supervisor's `BridgeEvent::Raw` arm, match
        `method == "codex/event/token_count"` and call
        `state.set_tokens(parse_codex_token_count(&params)?)`.
  - [ ] Match `method == "turn/started"` and call
        `state.incr_turn()`.
- [ ] Failures inside `parse_codex_token_count` log at debug and
      continue — never propagate. Observability shim must not
      affect run correctness.
- [ ] No changes to `BridgeEvent` variants. No changes to
      `runner/src/codex/bridge.rs::BridgeCursor::translate`.
- [ ] Claude path is a no-op. `tokens` and `turn_count` stay None
      for Claude during a run; `last_event_summary` will still
      populate from `Raw.method` like Codex.

### 1.7 Wire shape (`design.md` §4.2)

- [ ] In `runner/src/cloud/http.rs::PollStatus` (line 757), add the
      new optional fields per §4.2. Order: `observed_run_id` first
      (always serialized when feature on), then the rest with
      `#[serde(skip_serializing_if = "Option::is_none")]`.
- [ ] **Do not** modify `AttachBody`. Add a regression test that
      asserts `AttachBody`'s serialized field set is unchanged.
- [ ] Add `PollStatus::from_state(state: &StateHandle, in_flight:
    Option<Uuid>)` constructor that snapshots the state's
      mutexes once. `observed_run_id` is set from `in_flight` (=
      `rx_in_flight`), not from a separate field.
- [ ] Update the existing poll-time call site
      (`http.rs::poll_once`, currently constructs `PollStatus::from_wire`):
      switch to `PollStatus::from_state(...)` so the new fields
      flow on every poll.
- [ ] Feature-flag: when `agent_observability_v1=false`, all new
      fields serialize as absent (including `observed_run_id`).
      When true, `observed_run_id` is always serialized — possibly
      `null`.
- [ ] Unit test: feature off → wire bytes match the v3 (pre-this-
      design) shape exactly. Feature on, idle → `observed_run_id:
    null` is present, other fields absent. Feature on, busy →
      all populated fields present.

### 1.8 Phase A roll-out gate

- [ ] Add `daemon.agent_observability_v1: bool` to
      `runner/src/config/schema.rs::DaemonConfig`. Default false.
- [ ] CLI: `pidash status` shows the flag value (so operators can
      tell whether their fleet is reporting).
- [ ] Document in `runner/README.md` (one paragraph): the flag,
      what it costs (~250 bytes per poll), what the cloud needs
      (Phase B deployed) before turning it on.

## 2. Phase B — Cloud side

### 2.1 `RunnerLiveState` model (`design.md` §4.5.1)

- [ ] Add `RunnerLiveState` model to
      `apps/api/pi_dash/runner/models.py`:
  - [ ] `runner = OneToOneField(Runner, on_delete=CASCADE,
    primary_key=True, related_name="live_state")`
  - [ ] `observed_run_id = UUIDField(null=True, blank=True)`
  - [ ] `last_event_at = DateTimeField(null=True, blank=True)`
  - [ ] `last_event_kind = CharField(max_length=64, null=True, blank=True)`
  - [ ] `last_event_summary = CharField(max_length=200, null=True, blank=True)`
  - [ ] `agent_pid = PositiveIntegerField(null=True, blank=True)`
  - [ ] `agent_subprocess_alive = BooleanField(null=True, blank=True)`
  - [ ] `approvals_pending = PositiveSmallIntegerField(null=True, blank=True)`
  - [ ] `input_tokens = BigIntegerField(null=True, blank=True)`
  - [ ] `output_tokens = BigIntegerField(null=True, blank=True)`
  - [ ] `total_tokens = BigIntegerField(null=True, blank=True)`
  - [ ] `turn_count = PositiveIntegerField(null=True, blank=True)`
  - [ ] `updated_at = DateTimeField(auto_now=True)`
  - [ ] `Meta.indexes`: `Index(fields=["observed_run_id",
    "updated_at", "last_event_at"])`
- [ ] Migration `apps/api/pi_dash/runner/migrations/00XX_runner_live_state.py`:
      additive only, no data migration, no changes to existing
      tables. Depends on the latest existing runner migration.

### 2.2 Ingestion helper (`design.md` §4.5.2)

- [ ] Add to `apps/api/pi_dash/runner/services/session_service.py`:
  - [ ] `SNAPSHOT_FIELDS` tuple (does **not** include
        `observed_run_id` — that field drives the wipe, it is
        not a wipe target).
  - [ ] `parse_optional_uuid(raw)` — returns `None` for
        `None`/missing, raises `ValueError` for malformed UUIDs.
  - [ ] `upsert_runner_live_state(runner, status_entry)` per the
        §4.5.2 listing. Wipe-on-rid-change semantics. Tokens
        unpacking from nested `tokens.{input,output,total}` to
        the flat columns.
- [ ] In `apps/api/pi_dash/runner/views/sessions.py::RunnerSessionPollEndpoint.post`:
  - [ ] After the existing reaper / heartbeat update, call
        `upsert_runner_live_state(runner, body.get("status") or {})`.
- [ ] **Do not** modify `apply_hello`. Add a regression test
      asserting `apply_hello`'s behaviour is unchanged when the
      session-open body contains arbitrary extra fields.

### 2.3 Server-side stall watchdog (`design.md` §4.5.3)

- [ ] Add settings to `apps/api/apple_pi_dash/settings/common.py`:
  - [ ] `RUNNER_AGENT_STALL_THRESHOLD_SECS = 360`
  - [ ] `RUNNER_AGENT_OBSERVABILITY_STALE_SECS = 90`
- [ ] Add `reconcile_stalled_runs` Celery task to
      `apps/api/pi_dash/runner/tasks.py`. Query exactly as in
      §4.5.3, with the `models.F("id")` cross-join match,
      fresh-`updated_at` clause, and stale-`last_event_at` clause.
- [ ] Wire the task into Celery beat at the cadence specified
      (every 30s) — add to the project's beat schedule
      configuration.
- [ ] On stall match, call `run_lifecycle.finalize_run_terminal(
    run.runner, run.id, AgentRunStatus.FAILED, error_detail=
    f"agent stalled: no events for >{threshold}s")`. Same
      lifecycle helper used by other terminal transitions.

### 2.4 Cloud-side tests (`design.md` §5 Phase B.4)

- [ ] `test_upsert_runner_live_state`:
  - [ ] Pre-observability runner (status_entry empty) → no row
        written, no exception.
  - [ ] First snapshot for a runner → row created with the fields
        present.
  - [ ] Subsequent snapshot, same `observed_run_id` → only the
        present fields update; missing fields preserved.
  - [ ] `observed_run_id` change → all snapshot fields wiped to
        NULL **before** applying incoming values, in one save.
  - [ ] Idle clear (`observed_run_id: null`) → wipe persists, row
        retains the runner FK with NULL run binding.
  - [ ] Malformed UUID in `observed_run_id` → log warning,
        skip, do not raise.
- [ ] `test_reconcile_stalled_runs`:
  - [ ] Run with no `live_state` row → not reaped.
  - [ ] Run with `live_state.observed_run_id != run.id` → not
        reaped (snapshot describes a different run, e.g. previous
        completed run not yet overwritten).
  - [ ] Run with matching `observed_run_id` but stale
        `updated_at` (older than `RUNNER_AGENT_OBSERVABILITY_STALE_SECS`)
        → not reaped (runner stopped reporting; old data).
  - [ ] Run with matching `observed_run_id`, fresh `updated_at`,
        stale `last_event_at` (older than
        `RUNNER_AGENT_STALL_THRESHOLD_SECS`) → reaped, error
        message matches.
  - [ ] Run in `AWAITING_APPROVAL` / `AWAITING_REAUTH` → never
        reaped even when otherwise stale.
  - [ ] Pre-observability run with `last_event_at = NULL` → not
        reaped (NULL excluded by `__lt`).
- [ ] `test_session_poll_unchanged_for_old_runner`: a poll with
      no `status` payload (or one that omits all snapshot fields)
      leaves any pre-existing `live_state` row untouched.

### 2.5 Cloud-side observability metrics (optional)

- [ ] Increment a counter on each stall-watchdog firing for
      dashboarding (`pidash.runner.stall_reaped`). Tag with
      `agent_kind` so Codex / Claude rates can be compared.

## 3. Phase C — Operator surface

### 3.1 Web UI panel (`design.md` §4.5.4)

- [ ] Add `apps/web/core/components/runners/RunnerAgentStatusPanel.tsx`.
      Reads `Runner.live_state` plus the active `AgentRun`
      via the existing runner-detail query.
- [ ] Activity badge derivation, client-side only:
  - [ ] `null` everywhere → gray "unknown".
  - [ ] `agent_subprocess_alive == false` → red "dead".
  - [ ] `approvals_pending > 0` → blue "awaiting approval".
  - [ ] `last_event_at` within last 30s → green "active".
  - [ ] `last_event_at` 30-180s ago → yellow "thinking".
  - [ ] `last_event_at` > 180s ago → red "stalled".
- [ ] Render NULL fields per §4.5.4 table: "—" for scalar
      observability fields; tooltip hidden if `last_event_summary`
      is NULL.
- [ ] Storybook entry covering all six badge states + the
      all-null pre-observability state.

### 3.2 Read endpoint (optional)

- [ ] `GET /api/v1/runners/<rid>/live-state` returning the
      serialized snapshot. Auth: same as the existing
      runner-detail endpoint. Skip if the web UI's existing
      runner-detail endpoint can be extended instead — pick one
      surface, not both.

## 4. Test matrix (cross-phase)

End-to-end tests that exercise both runner and cloud against a
local docker stack:

- [ ] **Healthy run.** Runner accepts an Assign, agent emits
      events, run completes. Assert: `live_state` row has
      non-NULL `last_event_at` updated within the run, watchdog
      never fires, run reaches `AgentRunStatus.COMPLETED`.
- [ ] **Stalled agent.** Runner accepts Assign, agent process
      lives but emits no events for > threshold. Assert: cloud
      watchdog reaps the run with the documented error message;
      runner's existing internal stall watchdog also fires
      (overlap is expected and benign).
- [ ] **Daemon restart mid-run.** Restart `pidash` while a run is
      in flight. Assert: PR #94's drain step (commit `df687a6`)
      sends `RunFailed{daemon_restart}` first, the new
      `live_state` row's `observed_run_id` is cleared at restart's
      first poll, no spurious stall reap.
- [ ] **Feature flag off → on.** Boot a runner with
      `agent_observability_v1=false`, dispatch a run, then
      hot-reload the daemon with the flag on. Assert: first poll
      after restart populates `live_state`, watchdog behaves
      correctly thereafter.
- [ ] **Mixed fleet.** Two runners on the same connection, one
      with the flag on, one with the flag off. Assert: only the
      enabled runner's row is populated; the watchdog only fires
      on enabled runners.

## 5. Rollout plan

- [ ] Phase B lands first (additive cloud changes; no behavioural
      effect until runners send fields).
- [ ] Phase A lands second behind the flag.
- [ ] Per-cell rollout: enable `agent_observability_v1=true` on
      one fleet cell; observe `pidash.runner.stall_reaped` rate
      and the false-positive rate vs. the runner's internal
      5-minute watchdog firing rate. Tune
      `RUNNER_AGENT_STALL_THRESHOLD_SECS` if false positives
      appear.
- [ ] Phase C ships when at least one production cell has been
      running with the flag on for a week without watchdog
      false-positives.
- [ ] Default-enable the flag once the rollout reaches stable
      false-positive rate < 1% of stalled runs.

## 6. Disable / rollback plan

If the watchdog over-fires or the runner-side parsing is buggy:

- [ ] Set `agent_observability_v1=false` on affected runners.
      Existing in-flight runs continue normally; the next poll
      stops sending snapshot fields. The cloud's `live_state`
      row ages out via `RUNNER_AGENT_OBSERVABILITY_STALE_SECS`
      and the watchdog stops acting on those runners.
- [ ] To disable cloud-side watchdog without redeploying runners:
      remove the `reconcile_stalled_runs` task from Celery beat,
      or set `RUNNER_AGENT_STALL_THRESHOLD_SECS` to a very large
      number. The poll-handler ingestion is harmless to leave
      running.
- [ ] Migration is additive only; no rollback migration is
      needed. The `RunnerLiveState` table can be left in place
      during a temporary rollback; re-enabling is just flipping
      the runner flag back on.
