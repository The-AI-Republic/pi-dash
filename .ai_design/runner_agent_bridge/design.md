# Runner ↔ AI Agent Communication: Observability and Health Bridge

> Directory: `.ai_design/runner_agent_bridge/`
>
> Successor design layer on top of the HTTPS long-poll control plane
> (`.ai_design/move_to_https/design.md`). This proposal does **not**
> change the transport. It enriches what the runner tells the cloud
> about the AI agent subprocess (codex / claude) it bridges, and adds a
> server-side stall watchdog independent of the runner's own internal
> watchdog.
>
> Reference: OpenAI Symphony's elixir reference implementation
> (`/home/irichard/dev/study/symphony`) does this well; specific
> patterns we adopt are cited inline with file:line references.

## 1. Problem Statement

The runner exists for one reason: to be the bridge between the
pi-dash cloud and a locally-running AI agent CLI (codex or claude).
Operators care about the agent's health — _"is my codex run still
making progress?"_ — not the runner's. Today the heartbeat the cloud
sees only carries runner-level state:

```
{ status: "busy" | "idle", in_flight_run: <uuid> | null, ts: <iso> }
```

That is the minimum needed for the cloud's reaper to reconcile
"runner says it's working on X" with "cloud thinks it's working on X."
It is not enough to answer:

- Is the codex subprocess **alive** right now?
- Is it **actively producing tokens**, or has it gone silent?
- Is it **paused on an approval request** that needs operator
  attention?
- What is its **OS process id** so an operator can kill it from the
  host if necessary?
- How many **turns** has it consumed against the cap?
- How many **tokens** has it spent against the rate-limit window?

These signals are not all readily available on the runner today,
and that is itself part of the problem. The bridge layer
(`runner/src/agent/mod.rs:14-47`) only emits a small set of
agent-agnostic `BridgeEvent` variants — `RunStarted`, `Raw`,
`ApprovalRequest`, `AwaitingReauth`, `Completed`, `Failed` —
where everything not explicitly modelled (token counts, item
lifecycle, turn boundaries) is folded into `BridgeEvent::Raw {
method, params }`. For Codex, the `runner/src/codex/bridge.rs`
translator wraps notifications in `Raw` rather than promoting them
to discrete events; for Claude the `runner/src/claude_code/
bridge.rs` translator only surfaces token usage at all in the
terminal `Result` payload. So even at the supervisor layer, there
is no "this is a token update" or "this is a turn boundary" signal
to act on without re-parsing `Raw.params`. None of this surfaces to
the cloud, and operators see only a cloud-side `AgentRunStatus`
that flips every few seconds based on REST posts (`Accept`,
`RunStarted`, `ApprovalRequest`, etc.) plus a coarse runner-level
"busy/idle" badge. There is no continuous "agent CLI is alive and
producing" signal.

When the agent CLI hangs in a way that does _not_ close stdout — a
deadlocked tool call, a slow network in the underlying model API, an
unresponsive subprocess — the cloud finds out only after the runner's
own internal `STALL_TIMEOUT = 5min` watchdog fires
(`runner/src/daemon/supervisor.rs:933`). If the runner's tokio
runtime itself wedges, that watchdog never fires and the cloud has no
independent signal at all.

This design closes both gaps.

## 2. Reference: How Symphony Does It

Symphony's runner equivalent (the `Orchestrator`) maintains a rich
per-running-issue record that is updated on every codex frame and
exposed on a JSON observability endpoint plus a terminal dashboard.
Three patterns are directly applicable here.

### 2.1 Per-run live state, refreshed on every event

`elixir/lib/symphony_elixir/orchestrator.ex:1172-1199` — every codex
update flowing into the orchestrator is integrated into the running
entry:

```elixir
defp integrate_codex_update(running_entry, %{event: event, timestamp: timestamp} = update) do
  # ...
  Map.merge(running_entry, %{
    last_codex_timestamp: timestamp,
    last_codex_message: summarize_codex_update(update),
    session_id: session_id_for_update(running_entry.session_id, update),
    last_codex_event: event,
    codex_app_server_pid: codex_app_server_pid_for_update(...),
    codex_input_tokens: codex_input_tokens + token_delta.input_tokens,
    codex_output_tokens: codex_output_tokens + token_delta.output_tokens,
    codex_total_tokens: codex_total_tokens + token_delta.total_tokens,
    turn_count: turn_count_for_update(turn_count, running_entry.session_id, update)
  })
```

Every event the codex subprocess emits — token-usage updates, item
started, item completed, tool requests, turn boundaries — touches
this record. The orchestrator is therefore _always_ able to answer
"when did we last hear from this agent?" without polling the
subprocess directly.

### 2.2 Server-side stall watchdog

`elixir/lib/symphony_elixir/orchestrator.ex:448-487` — the
orchestrator's poll tick walks every running entry and compares
`now - last_codex_timestamp` against a configurable `stall_timeout_ms`:

```elixir
defp restart_stalled_issue(state, issue_id, running_entry, now, timeout_ms) do
  elapsed_ms = stall_elapsed_ms(running_entry, now)
  if is_integer(elapsed_ms) and elapsed_ms > timeout_ms do
    Logger.warning("Issue stalled: ... elapsed_ms=#{elapsed_ms}; restarting with backoff")
    state
    |> terminate_running_issue(issue_id, false)
    |> schedule_issue_retry(issue_id, next_attempt, %{
      identifier: identifier,
      error: "stalled for #{elapsed_ms}ms without codex activity"
    })
```

This runs _outside_ the agent worker process, so a wedged worker is
detected by the orchestrator regardless of whether the worker's own
internal timeout fires. **Belt-and-suspenders**: per-turn timeout in
the worker (`receive_loop` returns `{:error, :turn_timeout}` after
`turn_timeout_ms`) **and** server-side reconciliation in the
orchestrator. Either alone is insufficient.

### 2.3 Observability surface

`elixir/lib/symphony_elixir_web/presenter.ex:98-117` — the running
entry is projected to a JSON shape that the web UI and CLI dashboard
both consume:

```elixir
defp running_entry_payload(entry) do
  %{
    issue_id: entry.issue_id,
    issue_identifier: entry.identifier,
    state: entry.state,
    worker_host: Map.get(entry, :worker_host),
    workspace_path: Map.get(entry, :workspace_path),
    session_id: entry.session_id,
    turn_count: Map.get(entry, :turn_count, 0),
    last_event: entry.last_codex_event,
    last_message: summarize_message(entry.last_codex_message),
    started_at: iso8601(entry.started_at),
    last_event_at: iso8601(entry.last_codex_timestamp),
    tokens: %{
      input_tokens:  entry.codex_input_tokens,
      output_tokens: entry.codex_output_tokens,
      total_tokens:  entry.codex_total_tokens
    }
  }
end
```

`status_dashboard.ex:590-633` renders the same record on a terminal
UI with color-coded states keyed off `last_codex_event`:

```elixir
status_color =
  case event do
    :none                          -> @ansi_red
    "codex/event/token_count"      -> @ansi_yellow
    "codex/event/task_started"     -> @ansi_green
    "turn_completed"               -> @ansi_magenta
    _                              -> @ansi_blue
  end
```

The operator at a glance sees: PID, session, age + turn, total
tokens, last event class. Pi-dash's web UI can render the same.

## 3. Goals and Non-Goals

### 3.1 Goals

1. The cloud must continuously receive a **per-active-run**
   observability snapshot for the agent CLI subprocess — not just the
   runner's commitment-level status.
2. The cloud must be able to detect a stalled agent **without**
   relying on the runner's own internal stall watchdog.
3. The web UI must be able to show: agent state badge, last activity
   age, agent OS PID, turn / token consumption, pending approvals —
   all from one heartbeat snapshot.
4. New fields are additive and backward-compatible. Old runners
   without the new fields keep working; new runners against an old
   cloud also keep working.

### 3.2 Non-Goals

- Replacing the existing run-lifecycle REST endpoints (`Accept`,
  `RunStarted`, `ApprovalRequest`, etc.) — they remain the
  authoritative source of run-state transitions.
- Streaming live agent output to the cloud in real time — heartbeat
  cadence (~25s) is enough for state badges; per-event detail
  remains in run-lifecycle posts and the runner's local jsonl history.
- Adding remote operator commands ("kill agent", "interrupt"). That
  belongs in a follow-up; this design only adds the `agent_pid` that
  would enable such a command, not the command itself.
- Reimplementing the run state machine on the cloud side
  (`AgentRunStatus`).

## 4. Design

The design narrows Symphony's pattern to three things only:

1. update a live per-run snapshot on every agent event,
2. expose that snapshot to operators,
3. run a server-side stall watchdog independent of the runner's own
   internal watchdog.

What this design **does not** add:

- A synthetic `AgentLifecycle` enum competing with the existing
  `AgentRunStatus` and `RunnerStatus` enums. The cloud already has
  authoritative lifecycle transitions via run-lifecycle endpoints
  (`Accept`, `RunStarted`, `ApprovalRequest`, etc.); a third state
  machine on the runner that the cloud has to keep coherent with
  the first two creates more invariants than it pays for. Where the
  UI wants to render "the agent is thinking" or "the agent stalled,"
  it derives that from the descriptive scalars below
  (`last_event_at`, `agent_subprocess_alive`) — they are a
  presentation concern, not a state-machine concern.
- A reshape of the `BridgeEvent` API to introduce first-class
  `TokenUpdate` / `TurnBoundary` variants. The supervisor can
  opportunistically parse known `Raw.method` values for token
  deltas without changing the bridge contract, and that's enough
  for the observability story. (See §4.3.)
- New fields on session-open `AttachBody`. The snapshot only needs
  to flow on the poll path, which already runs every 25s. Session
  open is rare and is about identity + resume, not telemetry.
- Symphony's tracker orchestration model: polling Linear, selecting
  candidate issues, state/priority scheduling, blocker-aware
  dispatch, SSH worker-host routing, or multi-project coordination.
  Pi-dash already has its own run assignment and control-plane model;
  this design only borrows how Symphony supervises and observes an
  active agent process.

### 4.1 Heartbeat snapshot fields

Descriptive scalars only — no enums, no derived states. Each field
either has a current value or is `NULL` (unknown / not applicable).
The UI / watchdog interpret recency from `last_event_at` directly.

| Field                         | Type            | Source on the runner                                                                                                                                 |
| ----------------------------- | --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `observed_run_id`             | `Uuid?`         | The run id this snapshot describes (= `rx_in_flight`'s current value). Explicit `null` means idle / clear the row's run binding.                     |
| `last_event_at`               | `DateTime<Utc>` | Bumped on every `BridgeEvent` received in `pump_events`.                                                                                             |
| `last_event_kind`             | `String`        | Last frame's method/kind (e.g. `"codex/event/token_count"`, `"approval/request"`, `"turn/started"`); used by UI for status colour. Length-capped 64. |
| `last_event_summary`          | `String`        | One-line operator hint (e.g. `"tool/exec sh #14 (running 12s)"`). Length-capped 200; never carries prompt or model output content.                   |
| `agent_pid`                   | `u32`           | OS pid from `AgentBridge::process_handle().pid`; cleared on shutdown.                                                                                |
| `agent_subprocess_alive`      | `bool`          | Live answer from the bridge-owned exit notification (§4.4).                                                                                          |
| `approvals_pending`           | `u32`           | Existing `rx_approvals_pending`.                                                                                                                     |
| `tokens.{input,output,total}` | `u64`           | Optional. Codex only — supervisor opportunistically parses `codex/event/token_count` Raw frames. NULL for Claude (no streaming usage available).     |
| `turn_count`                  | `u32`           | Optional. Codex only — supervisor opportunistically increments on `turn/started`. NULL for Claude.                                                   |

The UI maps these scalars to its presentation:

- `last_event_at` recent → green badge ("active")
- `last_event_at` 30-180s old, `approvals_pending == 0` → yellow ("thinking")
- `last_event_at` > 180s old → red ("stalled")
- `agent_subprocess_alive == false` → red ("dead")
- `approvals_pending > 0` → blue ("awaiting approval")

These are CSS rules, not server-side state transitions. No
enum-keeping discipline required.

### 4.2 Where the snapshot rides — poll status only

The runner uses two different HTTP envelopes to talk to the cloud
(`AttachBody` at session-open, `PollStatus` nested under `"status"`
at every poll). The observability snapshot rides **only on the
poll**, not on session-open:

- Session-open is rare (one-shot at reconnect) and its job is
  identity + resume + bringing the runner online. The cloud already
  ingests it via `apply_hello` (`session_service.py:55`) which only
  cares about `version`, `os`, `arch`, `in_flight_run`. Mixing
  observability fields into that path doubles the ingestion surface
  and gives the cloud no information it doesn't see ~25s later from
  the next poll.
- Poll cadence is 25s by default. That is the natural rate for the
  observability snapshot to refresh — same cadence as the data, one
  ingestion path, one storage write.

Concretely, only `PollStatus` (`runner/src/cloud/http.rs:756-785`)
gains the new fields:

```rust
pub struct PollStatus {
    // existing
    pub status: String,
    pub in_flight_run: Option<Uuid>,
    pub ts: DateTime<Utc>,

    // NEW — observability snapshot. `observed_run_id` is always
    // serialized when the feature is enabled so the runner can
    // explicitly clear the cloud row by sending null when idle.
    pub observed_run_id: Option<Uuid>,

    // Remaining fields are optional descriptive scalars. Missing means
    // "do not change the existing value". Cross-run clears are driven by
    // observed_run_id changes, not by per-field nulls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_subprocess_alive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approvals_pending: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}
```

`AttachBody` is **not** extended. The feature flag controls whether
the runner serializes the new poll-status fields at all. When enabled,
`observed_run_id` is included even when it is `null`; when disabled,
none of the snapshot fields are sent.

Wire impact: ~200 bytes per poll. At a 25s cadence that is
~8 bytes/sec/runner — negligible.

### 4.3 Runner-side state plumbing

Add fields to `StateHandle::Inner` (`runner/src/daemon/state.rs`)
and wire watch channels for the volatile fields the transport layer
reads on each poll:

```rust
pub struct Inner {
    // ... existing ...
    last_event_at:        Mutex<Option<DateTime<Utc>>>,
    last_event_kind:      Mutex<Option<String>>,
    last_event_summary:   Mutex<Option<String>>,
    agent_pid:            Mutex<Option<u32>>,
    agent_subprocess_alive: Mutex<Option<bool>>,
    tokens:               Mutex<Option<TokenUsage>>,
    turn_count:           Mutex<Option<u32>>,
}
```

Helper API — designed so call sites stay narrow and there is no
state machine to keep coherent. Each setter is a small,
independent write:

```rust
impl StateHandle {
    /// Bump `last_event_at` and stamp `kind`/`summary`. Called from
    /// `pump_events` on every `BridgeEvent` regardless of variant.
    pub async fn note_agent_event(&self, ts: DateTime<Utc>, kind: &str, summary: Option<String>) { ... }

    pub async fn set_agent_pid(&self, pid: Option<u32>) { ... }
    pub async fn set_agent_alive(&self, alive: bool) { ... }
    pub async fn set_tokens(&self, usage: TokenUsage) { ... }
    pub async fn incr_turn(&self) { ... }

    /// Wipe the per-run snapshot before a *different* run begins.
    /// Invoked from `set_current_run(Some(_))` only when the run id
    /// changes, so a new run never inherits the previous run's
    /// `last_event_at`, tokens, turn count, or PID. Re-stamping the
    /// same run id during startup must not clear live values.
    pub async fn reset_run_snapshot(&self) {
        *self.inner.last_event_at.lock().await = None;
        *self.inner.last_event_kind.lock().await = None;
        *self.inner.last_event_summary.lock().await = None;
        *self.inner.agent_pid.lock().await = None;
        *self.inner.agent_subprocess_alive.lock().await = None;
        *self.inner.tokens.lock().await = None;
        *self.inner.turn_count.lock().await = None;
    }
}
```

`set_current_run(Some(...))` at `runner/src/daemon/state.rs:115`
calls `reset_run_snapshot()` only when the previous run id differs
from the next run id, then proceeds:

```rust
pub async fn set_current_run(&self, s: Option<CurrentRunSummary>) {
    let next = s.as_ref().map(|r| r.run_id);
    let prev = *self.tx_in_flight.borrow();
    if let (Some(next_id), Some(prev_id)) = (next, prev) {
        if next_id != prev_id {
            self.reset_run_snapshot().await;
        }
    } else if next.is_some() && prev.is_none() {
        // Idle → busy: clear any leftover snapshot from a prior run that
        // completed via `set_current_run(None)`.
        self.reset_run_snapshot().await;
    }
    // ... existing body ...
}
```

This preserves the startup invariant where the same run id may be
stamped more than once (supervisor's early stamp on Assign +
worker's later re-stamp at line 803 — see PR #94 commit `5e6b1c0`)
without flickering through idle. `set_current_run(None)` does not
reset local fields; it changes `observed_run_id` to `null` on the
next poll so the cloud can clear the live-state row's run binding.

Call sites — a small, mechanical list, with no protocol-detail
parsing in the supervisor unless the operator wants the optional
token/turn fields:

- `supervisor.rs::pump_events`, on every `BridgeEvent`: call
  `note_agent_event(Utc::now(), kind_of(&event), summary_of(&event))`.
  `kind_of` is a small one-line `match` on the `BridgeEvent` variant
  (and on `Raw.method` when the variant is `Raw`); `summary_of` is
  similarly a thin formatter.
- `supervisor.rs`, after `AgentBridge::spawn_*` returns: read
  `AgentBridge::process_handle().pid` and call
  `state.set_agent_pid(Some(pid))` when present. On shutdown,
  `set_agent_pid(None)`.
- `supervisor.rs`, on the process-exit notification described in
  §4.4: `state.set_agent_alive(false)` when the notification fires.
- Optional, opt-in via config flag — token / turn extraction.
  Today's `BridgeEvent::Raw` carries codex's `codex/event/token_count`
  and `turn/started` frames inline. The supervisor can match those
  methods and call `set_tokens` / `incr_turn` without the bridge API
  changing. This is **opportunistic observability** — when the agent
  exposes the data we forward it; when it doesn't (e.g. Claude during
  a run; codex of a future protocol version) we leave the fields
  NULL. There is no separate `BridgeEvent::TokenUpdate` /
  `TurnBoundary` variant introduced.

The supervisor having a small piece of agent-protocol knowledge
("if `Raw.method == codex/event/token_count`, parse usage") is
acceptable for an _observability_ shim. It is not a load-bearing
control-plane decision; if the parsing fails or the field shape
changes, we lose a chart pixel, not run correctness.

Heartbeat construction (`PollStatus::from_state(...)`) reads
`rx_in_flight` plus the snapshot fields — the same snapshotting
pattern as `current_attach_body()` from `http.rs`, extended to poll
status. `observed_run_id` is set from `rx_in_flight`, not from a
separate mutable field.

### 4.4 Independent process-exit watch

Today the bridge detects agent death only via stdout-close
(`pump_events` `bridge.next_events` returning None). On Linux this is
reliable for clean exits; for `kill -9` it usually is too, but a
malicious process could double-fork and we'd miss it.

Add an explicit bridge-owned exit notification. The supervisor cannot
call `child.wait()` directly today because the child is owned inside
`codex::app_server::AppServer` and `claude_code::process::
ClaudeProcess`. So the bridge layer grows a narrow process handle:

```rust
pub struct AgentProcessHandle {
    pub pid: Option<u32>,
    pub exit_rx: watch::Receiver<Option<ExitSnapshot>>,
}

pub struct ExitSnapshot {
    pub status_code: Option<i32>,
    pub signal: Option<i32>,
    pub observed_at: DateTime<Utc>,
}
```

Each concrete process wrapper spawns its own wait task immediately
after `cmd.spawn()` succeeds, updates `exit_rx` when the child exits,
and keeps stdout-reading behaviour unchanged. `AgentBridge` exposes
`process_handle()` so the supervisor can:

> **Implementation note — Child ownership refactor.**
> `tokio::process::Child::wait()` and `start_kill()` are both
> `&mut self`, so only one task can hold the `Child`. The existing
> `AppServer::shutdown()` at `runner/src/codex/app_server.rs:60-72`
> calls `self.child.wait()` and `self.child.start_kill()` directly
> on a `child: Child` field. To add the wait task, that field has
> to move into the wait task itself; the existing `AppServer`
> retains only a `kill_tx: oneshot::Sender<KillRequest>` (or
> `mpsc::Sender` for repeatable kills) plus the `exit_rx` from this
> design. `shutdown()` becomes "send a `KillRequest`, then await
> `exit_rx`," with the same overall timeout semantics. The wait
> task is the canonical Rust async-process pattern and is the only
> change of substance in `app_server.rs` /
> `claude_code/process.rs`; everything stdin/stdout side stays as
> it is.

- stamp `agent_pid`,
- mark `agent_subprocess_alive = true` after spawn,
- subscribe to `exit_rx` and mark `agent_subprocess_alive = false`
  when an exit snapshot arrives,
- let the existing `bridge.next_events(cursor) == None` path remain
  the terminal run-failure path.

This means the new watcher is observability-first. It does not add a
new authoritative `BridgeEvent::Dead` state or compete with the
existing stdout-close / `Failed` / `Completed` paths. If the process
exits before stdout closes, the UI can show "process exited" quickly;
the existing pump loop still finalizes the run through its current
control-flow. Symphony's analogue is the Erlang Port's
`{:exit_status, status}` message (`app_server.ex:356-357`), which is
native to the Port abstraction; Tokio requires us to expose the same
signal explicitly from the process wrapper.

### 4.5 Cloud-side reconciliation

#### 4.5.1 Storage — `RunnerLiveState`, keyed to the observed run

The cloud stores the snapshot in a dedicated `RunnerLiveState`
table — **not** on `Runner` (that would let a finished run's
metrics persist as worker-wide attributes), and **not** on
`AgentRun` (those columns would imply the data is part of the
authoritative run record, when it is in fact volatile and may be
overwritten / set NULL on every poll).

```python
# apps/api/pi_dash/runner/models.py
class RunnerLiveState(models.Model):
    runner = models.OneToOneField(
        Runner,
        on_delete=models.CASCADE,
        primary_key=True,
        related_name="live_state",
    )
    # The run this snapshot describes. NULL when the runner is idle.
    # The watchdog (§4.5.3) only acts when this matches a running
    # AgentRun's id — that is what makes the snapshot
    # unambiguously about the run we'd be failing.
    observed_run_id    = models.UUIDField(null=True, blank=True)
    last_event_at      = models.DateTimeField(null=True, blank=True)
    last_event_kind    = models.CharField(max_length=64,  null=True, blank=True)
    last_event_summary = models.CharField(max_length=200, null=True, blank=True)
    agent_pid          = models.PositiveIntegerField(null=True, blank=True)
    agent_subprocess_alive = models.BooleanField(null=True, blank=True)
    approvals_pending  = models.PositiveSmallIntegerField(null=True, blank=True)
    input_tokens       = models.BigIntegerField(null=True, blank=True)
    output_tokens      = models.BigIntegerField(null=True, blank=True)
    total_tokens       = models.BigIntegerField(null=True, blank=True)
    turn_count         = models.PositiveIntegerField(null=True, blank=True)
    updated_at         = models.DateTimeField(auto_now=True)

    class Meta:
        indexes = [
            # Watchdog query in §4.5.3 filters on observed_run_id,
            # updated_at freshness, and stale last_event_at.
            models.Index(fields=["observed_run_id", "updated_at", "last_event_at"]),
        ]
```

Properties this shape gives us:

- One row per `Runner` (`OneToOneField`); upserted on every poll.
  No need to look up the active `AgentRun` on each ingestion.
- `observed_run_id` is what makes the snapshot
  **unambiguously about a specific run.** When a new run starts,
  the next poll overwrites the row with the new run's id, blanking
  the old run's metrics in one write — so there is no carry-over
  even though storage isn't physically per-run.
- `RunnerLiveState` is treated as observability-only. Authoritative
  run state stays on `AgentRun` (status, assigned_at, started_at,
  ended_at, error). The two are intentionally not merged.

#### 4.5.2 Ingestion — poll handler only

Per §4.2, the snapshot rides only on `PollStatus`. So the only
ingestion site is `RunnerSessionPollEndpoint` at
`apps/api/pi_dash/runner/views/sessions.py:240-260`. `apply_hello`
does **not** change.

```python
# apps/api/pi_dash/runner/services/session_service.py (new helper)
SNAPSHOT_FIELDS = (
    "last_event_at", "last_event_kind",
    "last_event_summary", "agent_pid", "agent_subprocess_alive",
    "approvals_pending", "input_tokens", "output_tokens", "total_tokens",
    "turn_count",
)

def upsert_runner_live_state(runner, status_entry):
    """Apply the volatile observability snapshot from a poll body.

    `status_entry` is the dict the poll handler reads as
    body['status']. Missing fields are left as-is on the existing
    row — a stale poll never NULLs out a known-good value. A poll
    that carries a different
    `observed_run_id` than the row's current value saves a full wipe
    of every snapshot field before applying incoming values, ensuring
    no cross-run carryover.
    """
    if not status_entry:
        return
    state, _ = RunnerLiveState.objects.get_or_create(runner=runner)

    has_snapshot = "observed_run_id" in status_entry or any(
        key in status_entry for key in set(SNAPSHOT_FIELDS) | {"tokens"}
    )
    if not has_snapshot:
        return

    try:
        incoming_run_id = parse_optional_uuid(status_entry.get("observed_run_id"))
    except ValueError:
        logger.warning("ignoring runner live-state update with invalid observed_run_id")
        return
    update_fields = []

    if "observed_run_id" in status_entry and state.observed_run_id != incoming_run_id:
        # New run, or idle/null after a completed run. Persist the full
        # wipe, not just fields present on this poll.
        for f in SNAPSHOT_FIELDS:
            setattr(state, f, None)
        update_fields.extend(SNAPSHOT_FIELDS)
        state.observed_run_id = incoming_run_id
        update_fields.append("observed_run_id")

    for f in SNAPSHOT_FIELDS:
        if f in status_entry:
            setattr(state, f, status_entry[f])
            update_fields.append(f)
    if "tokens" in status_entry:
        tokens = status_entry["tokens"] or {}
        state.input_tokens = tokens.get("input")
        state.output_tokens = tokens.get("output")
        state.total_tokens = tokens.get("total")
        update_fields.extend(["input_tokens", "output_tokens", "total_tokens"])
    if update_fields:
        state.save(update_fields=sorted(set(update_fields)) + ["updated_at"])
```

The poll handler calls `upsert_runner_live_state(runner,
body.get('status') or {})` after the existing reaper / heartbeat
update. Pre-observability runners send no snapshot fields and the row
stays all-NULL / absent. New runners with the feature enabled always
send `observed_run_id`, so the cloud can clear the row's run binding
by receiving `null` when the runner goes idle.

#### 4.5.3 Server-side stall watchdog — explicit run-id match

A Celery beat task running every 30s. The watchdog requires both
that the snapshot's `observed_run_id` matches the run's id **and**
that the snapshot's `last_event_at` is older than the threshold.
It also requires a recently-updated `RunnerLiveState.updated_at`, so
the task only acts on runners that are still actively reporting the
observability snapshot. This avoids failing a run from a stale row if
the feature is disabled or a runner downgrades mid-run.
That match is the difference from the previous draft and the
reason a separate `RunnerLiveState` row earns its keep — it makes
"this snapshot belongs to _this_ run" expressible as a join
condition, not as an inference.

```python
# apps/api/pi_dash/runner/tasks.py
@app.task
def reconcile_stalled_runs():
    threshold = settings.RUNNER_AGENT_STALL_THRESHOLD_SECS  # default 360
    snapshot_freshness = settings.RUNNER_AGENT_OBSERVABILITY_STALE_SECS  # default 90
    now = timezone.now()
    cutoff = now - timedelta(seconds=threshold)
    snapshot_cutoff = now - timedelta(seconds=snapshot_freshness)

    # Active BUSY runs whose runner's snapshot:
    #   (a) currently describes this run (observed_run_id == run.id), AND
    #   (b) is still being reported by the runner, AND
    #   (c) hasn't recorded agent activity within the threshold.
    # NULL last_event_at is excluded by __lt; pre-observability
    # runners are therefore never reaped by this task.
    stalled = (
        AgentRun.objects
        .filter(status__in=BUSY_STATUSES)
        .exclude(status__in=(
            AgentRunStatus.AWAITING_APPROVAL,
            AgentRunStatus.AWAITING_REAUTH,
        ))
        .filter(
            runner__live_state__observed_run_id=models.F("id"),
            runner__live_state__updated_at__gte=snapshot_cutoff,
            runner__live_state__last_event_at__lt=cutoff,
        )
    )
    for run in stalled.select_related("runner"):
        run_lifecycle.finalize_run_terminal(
            run.runner, run.id, AgentRunStatus.FAILED,
            error_detail=f"agent stalled: no events for >{threshold}s",
        )
```

This covers the gap left by the existing heartbeat reaper:

- Runner's tokio runtime wedged: heartbeats stop, so the existing
  heartbeat reaper handles that via runner offline detection. The
  fresh-`updated_at` guard intentionally keeps this watchdog out of
  that failure mode.
- Agent CLI hung in a way that produces no events — the _new_ class
  this watchdog adds. Today the cloud only learns of these stalls
  after the runner's own 5-minute internal watchdog fires.
- Any future failure mode where the runner remains alive but the
  agent goes silent on a specific run.

Pre-observability runners pass through the watchdog cleanly: their
`live_state` row either doesn't exist or has all NULL fields, so
the inner `__lt` clause excludes their runs. Runners that previously
sent observability but stop sending it are also skipped once
`updated_at` ages past `RUNNER_AGENT_OBSERVABILITY_STALE_SECS`.

Default threshold: 360s. Configurable via Django settings,
overrideable per agent kind on the run's runner config.

#### 4.5.4 Web UI surface — observability only, not a second lifecycle

`apps/web/core/components/runners/...` gains a per-run detail
panel reading from `Runner.live_state` (the `RunnerLiveState` row
described in §4.5.1) joined with the active `AgentRun`. The panel
treats this data as **observability only** — it never overrides
the cloud-side `AgentRunStatus` for run lifecycle transitions, and
the badge it renders is derived from raw scalars, not from a
synthetic enum that the cloud has to agree with.

| UI element       | Source field                                                                                                   | NULL render      | Format                                                                  |
| ---------------- | -------------------------------------------------------------------------------------------------------------- | ---------------- | ----------------------------------------------------------------------- |
| Activity badge   | derived from `last_event_at` + `agent_subprocess_alive` + `approvals_pending` (CSS rule, not server-side enum) | "unknown" (gray) | green=active, yellow=thinking, red=stalled/dead, blue=awaiting approval |
| Last activity    | `last_event_at`                                                                                                | "—"              | "12s ago", "2m ago"                                                     |
| Last event kind  | `last_event_kind`                                                                                              | "—"              | small label (e.g. `tool/exec`)                                          |
| Agent PID        | `agent_pid`                                                                                                    | "—"              | numeric, with copy button                                               |
| Subprocess alive | `agent_subprocess_alive`                                                                                       | "—"              | "yes" / "no" / "—"                                                      |
| Approvals        | `approvals_pending`                                                                                            | "0"              | numeric, with link to approvals view                                    |
| Tokens           | `total_tokens` (and i/o split)                                                                                 | "—"              | "31.0k"                                                                 |
| Turn             | `turn_count`                                                                                                   | "—"              | numeric                                                                 |
| Last event       | `last_event_summary`                                                                                           | hidden           | tooltip                                                                 |

The activity badge is the only place the UI synthesises a state
label, and it does so on the client from the raw scalars. There is
no server-side `agent_state` enum to keep coherent with
`AgentRunStatus` and `RunnerStatus`. This is what Codex's review
of v2 of this doc was pointing at: keep observability descriptive,
not authoritative.

This is roughly what Symphony's terminal dashboard renders
(`status_dashboard.ex:590-633`), translated to a web component —
but with the lifecycle classification done on the client side
rather than persisted as state.

## 5. Phasing

### Phase A — Runner-side, no cloud changes

Lands behind a feature flag. Heartbeat carries the new fields if
the config opts in. Cloud ignores them today.

A.1 `RunnerLiveState`-style scalar fields on `StateHandle::Inner`
(no `AgentLifecycle` enum).
A.2 `note_agent_event(ts, kind, summary)` / `set_agent_pid` /
`set_agent_alive` wired in `pump_events` and bridge spawn.
A.3 `PollStatus` extended with the optional snapshot fields
(`AttachBody` is **not** changed).
A.4 Bridge-owned process-exit notification (§4.4) drives
`agent_subprocess_alive`; the supervisor subscribes to it but does
not own `child.wait()` directly.
A.5 Optional, opt-in: supervisor-side parsing of
`codex/event/token_count` and `turn/started` from `Raw.method` to
populate `tokens` / `turn_count`. No `BridgeEvent` API change.
A.6 `reset_run_snapshot()` invoked from `set_current_run(Some(_))`
only when the run id changes, so a new run never sends stale fields
and a same-run re-stamp does not erase live values.
A.7 Unit tests for the snapshot-reset on new-run boundary, the
process-exit watcher, and PollStatus serialisation.

Ships behind `agent_observability_v1` config flag, default off.

### Phase B — Cloud-side ingestion

B.1 DB migration: new `RunnerLiveState` table with the
`(observed_run_id, updated_at, last_event_at)` index for the watchdog.
B.2 `upsert_runner_live_state(runner, status_entry)` helper called
from the poll handler **only** (`apply_hello` is unchanged). The
helper persists a full snapshot wipe on `observed_run_id` change.
B.3 Server-side stall reconcile Celery task with the explicit
`observed_run_id == agent_run.id` join.
B.4 Tests covering: backward compat (no `live_state` row,
all-NULL row), `observed_run_id` change persists a full wipe before
applying incoming values, idle/null clears the row's run binding,
stall watchdog requires run-id match, fresh `updated_at`, and stale
`last_event_at`, AwaitingApproval/AwaitingReauth excluded.

When B ships, runners can flip the flag on; old runners continue
to work without the new fields. An old run that completes on a
now-upgraded fleet does **not** poison the next run's metrics
because `upsert_runner_live_state` wipes the snapshot the moment
`observed_run_id` changes.

### Phase C — Operator surface

C.1 Web UI per-run detail panel; activity badge derived
client-side from raw scalars.
C.2 Optional JSON observability endpoint
(`GET /api/v1/runners/<rid>/live-state`) for external dashboards.
C.3 Status colour in the runs list view, from `last_event_at`
recency.

## 6. Backward and Forward Compatibility

`RunnerLiveState` columns are nullable end-to-end; `NULL` is the
canonical "unknown" sentinel for both the watchdog and the UI.
There is no `default="idle"` and no `default=0` — those would
silently misrepresent pre-observability runs as "idle, 0 tokens"
instead of "unknown."

| Direction                        | Effect                                                                                                                                                |
| -------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| New runner → old cloud           | Old cloud ignores unknown fields in `PollStatus`; existing reaper still works on `in_flight_run`. `AttachBody` is unchanged.                          |
| Old runner → new cloud           | Heartbeat carries no snapshot fields; `live_state` row stays NULL or absent; UI renders "—"; stall watchdog skips the run.                            |
| New runner → new cloud, flag off | Heartbeat carries no snapshot fields; any old `live_state` row ages out via the watchdog's `updated_at` freshness guard.                              |
| Mixed fleet during rollout       | Each runner is independent. A new run starts by overwriting the snapshot via `observed_run_id` change, blanking the previous run's data in one write. |

Field stability:

- All snapshot fields are descriptive scalars; there is no
  enum-typed field whose variant set the cloud has to keep current
  with the runner. New scalars can be added later without breaking
  parsing on either side.
- `tokens` / `turn_count` may be NULL during a run (e.g. Claude has
  no streaming tokens); ingestion preserves the previous non-NULL
  value rather than blanking it. The exception is the
  `observed_run_id`-changed wipe, which clears all snapshot fields
  in one write.
- A heartbeat that omits the entire snapshot block (old runner, or
  new runner with the flag off) leaves the `live_state` row
  unchanged. The watchdog only acts when `updated_at` is fresh, so a
  stale row from an old/disabled runner ages out instead of failing an
  active run from old data. The explicit run-id match still protects
  the normal completed-run case where a previous run's snapshot is
  present but no longer describes any active run.

## 7. Risks and Open Questions

1. **Threshold tuning.** A 360s server-side stall threshold may be
   wrong for slow-agent flows (e.g., a long reasoning model). Start
   conservative (longer than the runner's own 5min internal watchdog)
   and tune per agent kind via the `agent.<kind>.stall_threshold_secs`
   config knob.

2. **Soft warn vs. hard fail when stalls cross the threshold.**
   The runner's internal watchdog already fails the run at 5
   minutes, and the cloud's stall reconcile (§4.5.3) also fails it.
   The runner-side action is the source of truth in the current
   design; the cloud side is a backstop for cases where the
   runner-side watchdog can't fire (wedged tokio, OS hang). If we
   later want to _retry_ stalled runs instead of failing them, the
   cloud is the right place — it has the retry policy context the
   runner does not. Out of scope for v1.

3. **Privacy of `last_event_summary`.** Must never include prompt
   text, model output, or file contents. The runner's
   `summary_of(BridgeEvent)` formatter sanitises by structure: only
   method name, item id, duration, exit code. Codex tool args
   containing user input are out of scope — keep at summary level.

4. **PID stability under SSH-driven workers.** Symphony runs codex
   on remote workers via SSH; the PID it reports is the SSH parent
   PID, not the codex PID directly. Pi-dash today runs the agent
   locally, so this isn't an issue. If we add SSH-driven workers
   later, the PID field needs a host-qualifier.

5. **`approvals_pending` already exists on the runner side
   (`set_approvals_pending`).** Just wire it through the heartbeat;
   no new state needed.

6. **Cloud-side stall reconcile cost.** O(active_runs) per 30s
   tick, with the `(observed_run_id, updated_at, last_event_at)`
   index keeping the join cheap. Trivial at expected scale (≤ thousands of
   concurrent runs); revisit if scale changes.

7. **Snapshot stalemate when the runner upgrades mid-run.** If a
   pre-observability runner runs to completion and then upgrades
   without finishing, its `live_state` row is empty. After upgrade
   the next run's first poll writes the new `observed_run_id` and
   the system is healthy. Old completed runs are never re-evaluated
   by the watchdog because they aren't in `BUSY_STATUSES`. So this
   case is benign.

8. **Should pi-dash also adopt Symphony's multi-turn continuation?**
   Symphony retries the same issue across multiple codex turns up
   to a `max_turns` cap; pi-dash today fails the run on first agent
   termination. Out of scope for this design — flag as a separate
   exploration.

## 8. Open Items for Future Designs

- **Remote operator commands** using `agent_pid`: "interrupt agent",
  "kill agent", "send signal." Requires a runner-side IPC handler
  and cloud-side authorization.
- **Live event streaming**: the existing per-event REST posts could
  be replaced by an SSE / WebSocket per-run upgrade for verbose runs,
  reusing the WS protocol kept dormant in
  `.ai_design/move_to_https/design.md`.
- **Multi-turn continuation** like Symphony's
  `do_run_codex_turns` (issue stays active → continuation prompt →
  next turn). Today pi-dash treats every assignment as one-shot.

## 9. Summary

Today the runner reports a four-bit picture of its state to the
cloud: alive, busy, on-run-X, idle. That is enough for the cloud to
reconcile commitment but not for the operator to reason about the
agent. Symphony's reference implementation shows a richer
agent-process-management pattern that costs ~250 bytes per heartbeat:
per-event activity timestamp, agent PID, turn count, token usage, and
operator-facing status projection. Pi-dash adopts that
observability/watchdog pattern without adopting Symphony's tracker
scheduler or lifecycle vocabulary. Bringing the narrowed pattern into
pi-dash gives:

1. The cloud a way to detect stalled agents without depending on the
   runner's own internal watchdog.
2. The web UI enough material to render meaningful per-run status
   beyond "running" or "failed."
3. Operators a PID they can act on when a local subprocess
   misbehaves.

The change is additive on both sides, behind a feature flag, with
clean fallbacks for mixed fleets.

---

## Appendix A — Symphony reference index

| Pattern                      | Symphony source                                                                       |
| ---------------------------- | ------------------------------------------------------------------------------------- |
| Per-run rich state record    | `elixir/lib/symphony_elixir/orchestrator.ex:1172-1199`                                |
| Server-side stall reconcile  | `elixir/lib/symphony_elixir/orchestrator.ex:448-487`                                  |
| Process-exit detection       | `elixir/lib/symphony_elixir/codex/app_server.ex:356-357` (Erlang Port `:exit_status`) |
| Codex frame dispatch         | `elixir/lib/symphony_elixir/codex/app_server.ex:364-438`                              |
| Token/turn tracking          | `elixir/lib/symphony_elixir/orchestrator.ex:1172-1230`                                |
| JSON observability shape     | `elixir/lib/symphony_elixir_web/presenter.ex:98-117`                                  |
| Terminal dashboard rendering | `elixir/lib/symphony_elixir/status_dashboard.ex:590-633`                              |

## Appendix B — Pi-dash insertion-point index

| Component              | File                                                                   | Change                                                                                                                              |
| ---------------------- | ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| State store            | `runner/src/daemon/state.rs`                                           | add scalar fields (no lifecycle enum); `note_agent_event(ts, kind, summary)`; `reset_run_snapshot` invoked only when run id changes |
| Bridge process handle  | `runner/src/agent/mod.rs::AgentBridge`                                 | expose `process_handle() -> AgentProcessHandle` with PID + exit notification                                                        |
| Process-exit watcher   | `runner/src/codex/app_server.rs`, `runner/src/claude_code/process.rs`  | bridge-owned wait task updates exit notification; supervisor subscribes and drives `set_agent_alive(false)`                         |
| Event ingestion        | `runner/src/daemon/supervisor.rs::pump_events`                         | call `note_agent_event` on every `BridgeEvent`; opportunistic `Raw.method` matching for tokens / turn (opt-in)                      |
| PollStatus (poll only) | `runner/src/cloud/http.rs::PollStatus` (line 757)                      | add the optional snapshot fields. **`AttachBody` is unchanged**                                                                     |
| Poll handler ingestion | `apps/api/pi_dash/runner/views/sessions.py::RunnerSessionPollEndpoint` | call new `upsert_runner_live_state(runner, body.get("status") or {})`                                                               |
| Live-state model       | `apps/api/pi_dash/runner/models.py::RunnerLiveState` (new)             | one row per runner, all nullable; keyed to `observed_run_id`; `(observed_run_id, updated_at, last_event_at)` index                  |
| Stall reconcile        | `apps/api/pi_dash/runner/tasks.py::reconcile_stalled_runs` (new)       | Celery beat; explicit `live_state.observed_run_id == agent_run.id` join + fresh `updated_at` + stale `last_event_at`                |
| DB migration           | `apps/api/pi_dash/runner/migrations/00XX_runner_live_state.py`         | additive — new table only, no changes to `AgentRun` / `Runner`                                                                      |
| UI panel               | `apps/web/core/components/runners/RunnerAgentStatusPanel.tsx` (new)    | observability-only; activity badge derived client-side from raw scalars; NULLs render as "—"                                        |
