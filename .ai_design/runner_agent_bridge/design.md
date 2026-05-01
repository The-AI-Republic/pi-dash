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

These signals exist _internally_ on the runner — the supervisor and
the bridge see them every time a `BridgeEvent` flows past — but they
never reach the cloud, and they never reach the operator's UI. The
operator gets a cloud-side `AgentRunStatus` that flips every few
seconds based on REST endpoint posts (`Accept`, `RunStarted`,
`ApprovalRequest`, etc.) and a coarse runner-level "busy/idle" badge.
There is no continuous "agent CLI is alive and producing" signal.

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

1. The cloud's per-runner state in the DB must continuously reflect
   what the agent CLI is doing — not just the runner's
   commitment-level status.
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
  belongs in a follow-up; this design only adds the agent_pid that
  would _enable_ such a command, not the command itself.
- Reimplementing the run state machine on the cloud side
  (`AgentRunStatus`).

## 4. Design

### 4.1 Agent lifecycle vocabulary

A new enum on the runner side, mirroring the natural states the
bridge already transitions through:

```rust
// runner/src/agent/lifecycle.rs (new)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycle {
    /// Workspace setup is running; the agent CLI subprocess has not been
    /// spawned yet. Covers the gap between Assign acceptance and bridge
    /// spawn (currently the period that triggered bug #2 in PR #94).
    Preparing,
    /// Subprocess has been spawned; we have an OS PID; no events yet.
    Initializing,
    /// Events are flowing from the subprocess (last_event_at is recent).
    Streaming,
    /// Subprocess is alive but has gone quiet (within stall threshold).
    /// Surfaced separately from Streaming so the UI can render
    /// "thinking…" without alarming the operator.
    Thinking,
    /// Paused on an approval request (cloud or operator approval pending).
    AwaitingApproval,
    /// Paused on an agent-side reauth flow.
    AwaitingReauth,
    /// No events in [stall_threshold, dead_threshold) — soft warning,
    /// not yet failed. The cloud's stall watchdog acts on this.
    Stalled,
    /// Subprocess emitted Completed; runner is finalising before sending
    /// RunCompleted to the cloud.
    Completing,
    /// Subprocess process has exited but the run hasn't been finalised on
    /// the cloud yet. Should be brief; if it lingers, it indicates a bug
    /// in the runner's failure path.
    Dead,
    /// Default before any run is in flight.
    Idle,
}
```

Drive the state machine from `pump_events`:

| Trigger                                                      | New lifecycle      |
| ------------------------------------------------------------ | ------------------ |
| Assign accepted, before workspace resolve                    | `Preparing`        |
| `AgentBridge::spawn_from_config` returns Ok with a child PID | `Initializing`     |
| First `BridgeEvent::Raw` received from the bridge            | `Streaming`        |
| > 30s since last event, no approval pending                  | `Thinking`         |
| `BridgeEvent::ApprovalRequest` (and not yet Decided)         | `AwaitingApproval` |
| `BridgeEvent::AwaitingReauth`                                | `AwaitingReauth`   |
| > `STALL_THRESHOLD` (default 180s) since last event          | `Stalled`          |
| `BridgeEvent::Completed`                                     | `Completing`       |
| Bridge stdout closes / child exit observed                   | `Dead`             |
| Run terminated and `set_current_run(None)`                   | `Idle`             |

The thresholds (30s for Thinking, 180s for Stalled) are tunable via
`Config::settings.codex` / `claude_code` sections so they can differ
per agent kind.

### 4.2 Heartbeat schema extension

`runner/src/cloud/http.rs::PollStatus` and the `AttachBody` it
mirrors get optional new fields. All optional so both directions of
mismatch (old runner ↔ new cloud, new runner ↔ old cloud) keep
parsing.

```rust
pub struct PollStatus {
    pub status: String,                          // existing
    pub in_flight_run: Option<Uuid>,             // existing
    pub ts: DateTime<Utc>,                       // existing

    // NEW — all optional, all derived from runner state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_state: Option<AgentLifecycle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approvals_pending: Option<u32>,
    /// One-line human summary of the most recent event, e.g.
    /// "tool/exec sh #14 (running 12s)". Length-capped to 200 chars to
    /// keep heartbeat size bounded. Never includes prompt content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}
```

Wire impact: ~150-250 bytes per poll body. With 25s default cadence
that is ~10 bytes/sec/runner — negligible against existing payload
sizes.

### 4.3 Source of truth on the runner side

Add fields to `StateHandle::Inner` (`runner/src/daemon/state.rs`)
and wire watch channels for the volatile fields the transport layer
reads each poll:

```rust
pub struct Inner {
    // ... existing ...
    agent_lifecycle: Mutex<AgentLifecycle>,
    agent_pid: Mutex<Option<u32>>,
    last_event_at: Mutex<Option<DateTime<Utc>>>,
    turn_count: Mutex<u32>,
    tokens: Mutex<TokenUsage>,
}

pub struct StateHandle {
    // ... existing ...
    pub tx_agent_lifecycle: watch::Sender<AgentLifecycle>,
    pub rx_agent_lifecycle: watch::Receiver<AgentLifecycle>,
    pub tx_last_event_at: watch::Sender<Option<DateTime<Utc>>>,
    pub rx_last_event_at: watch::Receiver<Option<DateTime<Utc>>>,
    // ... etc.
}
```

Mutating helpers:

```rust
impl StateHandle {
    pub async fn note_agent_event(&self, ts: DateTime<Utc>) {
        self.tx_last_event_at.send_replace(Some(ts));
        // Auto-promote Thinking → Streaming on any event.
        self.tx_agent_lifecycle.send_if_modified(|cur| {
            if matches!(*cur, AgentLifecycle::Thinking | AgentLifecycle::Initializing) {
                *cur = AgentLifecycle::Streaming;
                true
            } else { false }
        });
    }
    pub async fn set_agent_lifecycle(&self, next: AgentLifecycle) { ... }
    pub async fn set_agent_pid(&self, pid: Option<u32>) { ... }
    pub async fn add_tokens(&self, delta: TokenUsage) { ... }
    pub async fn incr_turn(&self) { ... }
}
```

Call sites:

- `supervisor.rs::pump_events` — call `note_agent_event(now)` on every
  `BridgeEvent` received.
- `agent::Bridge::spawn_*` — return the child PID; supervisor calls
  `set_agent_pid(Some(pid))` after spawn, `set_agent_pid(None)` on
  shutdown.
- `codex::bridge` — when a `token_count` codex frame arrives, call
  `add_tokens(delta)`.
- `codex::bridge` — when a turn boundary is observed (codex's
  `turn/started` or `session_started`-with-new-thread), call
  `incr_turn()`.
- A new tokio interval task in the supervisor checks
  `now - last_event_at` periodically and flips the lifecycle to
  `Thinking` / `Stalled` accordingly.

Heartbeat construction (`PollStatus::from_state(...)`) reads the
watch receivers — the existing pattern from `current_attach_body()`
extended to the new fields.

### 4.4 Independent process-exit watch

Today the bridge detects agent death only via stdout-close
(`pump_events` `bridge.next_events` returning None). On Linux this is
reliable for clean exits; for `kill -9` it usually is too, but a
malicious process could double-fork and we'd miss it.

Add a parallel watcher: when the bridge spawns the child, the
supervisor also tasks a `tokio::spawn(async move { child.wait().await })`
that posts `BridgeEvent::Dead { exit_status }` into the same event
stream. First-wins between this and stdout-close drives the run to
its terminal state. Symphony's analogue is the Erlang Port's
`{:exit_status, status}` message
(`app_server.ex:356-357`), which ships natively with the Port
abstraction; we have to wire it explicitly because Tokio doesn't.

### 4.5 Cloud-side reconciliation

#### 4.5.1 Persist the heartbeat fields

Add columns to `Runner` (or a new `RunnerLiveState` row updated
per-poll):

```python
# apps/api/pi_dash/runner/models.py — Runner extensions
agent_lifecycle = models.CharField(max_length=32, default="idle")
agent_pid = models.IntegerField(null=True, blank=True)
last_event_at = models.DateTimeField(null=True, blank=True)
agent_turn_count = models.PositiveIntegerField(default=0)
agent_input_tokens = models.PositiveIntegerField(default=0)
agent_output_tokens = models.PositiveIntegerField(default=0)
agent_total_tokens = models.PositiveIntegerField(default=0)
agent_approvals_pending = models.PositiveSmallIntegerField(default=0)
agent_last_event_summary = models.CharField(max_length=200, blank=True, default="")
```

Migration is additive; existing rows fill with defaults. A separate
denormalised table is also reasonable if we prefer to keep the
`Runner` row append-rare; either is acceptable.

`apply_hello` and the poll endpoint upsert these from the heartbeat
payload, defaulting to `None`/`0` if the field is absent (old
runner).

#### 4.5.2 Server-side stall watchdog

A Celery beat task running every 30s — Symphony's
`reconcile_stalled_running_issues` analogue:

```python
# apps/api/pi_dash/runner/tasks.py
@app.task
def reconcile_stalled_runs():
    threshold = settings.RUNNER_AGENT_STALL_THRESHOLD_SECS  # default 300
    cutoff = timezone.now() - timedelta(seconds=threshold)

    stalled_runs = AgentRun.objects.filter(
        status__in=BUSY_STATUSES,
        runner__last_event_at__lt=cutoff,  # new column
    ).exclude(
        # Don't fail runs that are legitimately paused. AwaitingApproval
        # and AwaitingReauth runs are *expected* to have no codex frames.
        status__in=(AgentRunStatus.AWAITING_APPROVAL, AgentRunStatus.AWAITING_REAUTH),
    )
    for run in stalled_runs:
        run_lifecycle.finalize_run_terminal(
            run.runner, run.id, AgentRunStatus.FAILED,
            error_detail=f"agent stalled: no events for >{threshold}s",
        )
```

This is independent of the heartbeat reaper. It catches:

- Runner's tokio runtime wedged (heartbeats stop, but reaper has its
  own offline detection that handles that case).
- Agent CLI hung in a way that produces no events (this is the
  _new_ class — today it has to wait for the runner's internal 5min
  stall watchdog).
- Any future failure mode where the runner remains alive but the
  agent goes silent.

Default threshold: 300s. Configurable via Django settings, also
overrideable per-runner via a column for deployments with
intentionally slow agents.

#### 4.5.3 Web UI surface

`apps/web/core/components/runners/...` gains a per-runner detail
panel reading from the heartbeat:

| UI element        | Source field                    | Format                                                                       |
| ----------------- | ------------------------------- | ---------------------------------------------------------------------------- |
| Agent state badge | `agent_lifecycle`               | colored chip (green=streaming, yellow=thinking, red=stalled/dead, blue=idle) |
| Last activity     | `last_event_at`                 | "12s ago", "2m ago"                                                          |
| Agent PID         | `agent_pid`                     | numeric, with copy button                                                    |
| Turn              | `agent_turn_count` / config max | "2 / 5"                                                                      |
| Tokens            | `agent_total_tokens`            | "31.0k"                                                                      |
| Approvals         | `agent_approvals_pending`       | numeric, with link to approvals view                                         |
| Last event        | `agent_last_event_summary`      | tooltip                                                                      |

This is roughly what Symphony's terminal dashboard renders
(`status_dashboard.ex:590-633`), translated to a web component.

## 5. Phasing

### Phase A — Runner-side, no cloud changes

Lands behind a feature flag. Heartbeat carries the new fields if the
config opts in. Cloud ignores them today.

A.1 `AgentLifecycle` enum + watch channel on `StateHandle`.
A.2 `note_agent_event` / `set_agent_pid` / lifecycle transitions
wired in `pump_events` and bridge spawn.
A.3 `tokens` and `turn_count` extracted from codex frames in
`codex::bridge`.
A.4 `PollStatus` and `AttachBody` extended with optional fields.
A.5 Independent process-exit watcher.
A.6 Unit tests for state transitions.

Ships behind `agent_observability_v1` config flag, default off.

### Phase B — Cloud-side ingestion

B.1 DB migration for the new `Runner` columns.
B.2 `apply_hello` + poll endpoint upsert.
B.3 Server-side stall reconcile Celery task.
B.4 Tests covering: backward compat (old heartbeat shape),
stall detection true positive / false positive, awaiting-approval
suppression.

When B ships, runners can flip the flag on; old runners continue to
work without the new fields.

### Phase C — Operator surface

C.1 Web UI runner detail panel.
C.2 Optional JSON observability endpoint
(`GET /api/v1/runners/<rid>/agent-status`) for external dashboards.
C.3 Status badges in the runs list view (color of last activity).

## 6. Backward and Forward Compatibility

| Direction                        | Effect                                                                                                       |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| New runner → old cloud           | Old cloud ignores unknown fields; reaper still works on the existing `in_flight_run` field.                  |
| Old runner → new cloud           | New cloud sees the heartbeat fields as `None`; stall reconcile skips runs whose `last_event_at` is `NULL`.   |
| New runner → new cloud, flag off | Heartbeat carries no new fields; cloud falls back to existing reaper rules.                                  |
| Mixed fleet during rollout       | Each runner is independent; the cloud's per-runner reconciliation never compares runners against each other. |

Field stability:

- `AgentLifecycle` is serialized as snake_case strings; new variants
  may be added; the cloud parses with a default fallback to
  `"unknown"` so it never crashes on a future variant.
- `tokens` / `turn_count` are monotonic per session; the cloud
  resets them when a new `session_id` is observed.

## 7. Risks and Open Questions

1. **Threshold tuning.** A 300s server-side stall threshold may be
   wrong for slow-agent flows (e.g., a long reasoning model). Start
   conservative (longer than the runner's own 5min internal watchdog)
   and tune per agent kind via the `agent.<kind>.stall_threshold_secs`
   config knob.

2. **`Stalled` lifecycle as a soft state vs. hard fail.** The runner's
   internal watchdog _fails_ the run after 5 minutes; the cloud's
   reconcile task does the same. Should the runner instead surface
   `Stalled` and let the cloud decide? Likely yes, because the cloud
   has policy context (retry budgets, user preferences) the runner
   does not. This design preserves both — the runner reports the
   warning state, both sides can independently act.

3. **Privacy of `last_event_summary`.** Must never include prompt
   text, model output, or file contents. Sanitiser layer in the
   runner: only carry method name, item id, duration, exit code.
   Codex tool args containing user input are out of scope — keep at
   summary level.

4. **PID stability under SSH-driven workers.** Symphony runs codex on
   remote workers via SSH; the PID it reports is the SSH parent PID,
   not the codex PID directly. Pi-dash today runs the agent locally,
   so this isn't an issue. If we add SSH-driven workers later, the
   PID field needs a host-qualifier.

5. **`approvals_pending` already exists on the runner side
   (`set_approvals_pending`).** Just wire it through the heartbeat;
   no new state needed.

6. **Cloud-side stall reconcile cost.** O(running_runs) per 30s
   tick. Index on `(status, last_event_at)`. Trivial at expected
   scale (≤ thousands of concurrent runs); revisit if scale changes.

7. **Should pi-dash also adopt Symphony's `turn_count`-based
   continuation?** Symphony retries the same issue across multiple
   codex turns up to a `max_turns` cap; pi-dash today fails the run
   on first agent termination. Out of scope for this design — flag as
   a separate exploration.

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
agent. Symphony's reference implementation shows a richer pattern
that costs ~250 bytes per heartbeat: per-event activity timestamp,
agent PID, turn count, token usage, and a small lifecycle vocabulary.
Bringing the same pattern into pi-dash gives:

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

| Component       | File                                                                    | Change                       |
| --------------- | ----------------------------------------------------------------------- | ---------------------------- |
| Lifecycle enum  | `runner/src/agent/lifecycle.rs` (new)                                   | new module                   |
| State store     | `runner/src/daemon/state.rs`                                            | add fields + setters         |
| Event ingestion | `runner/src/daemon/supervisor.rs::pump_events`                          | call `note_agent_event`      |
| Bridge PID      | `runner/src/agent/mod.rs::AgentBridge`                                  | expose `child_pid()`         |
| Heartbeat shape | `runner/src/cloud/http.rs::PollStatus` + `AttachBody`                   | add optional fields          |
| Hello apply     | `apps/api/pi_dash/runner/services/session_service.py::apply_hello`      | upsert new fields            |
| Poll handler    | `apps/api/pi_dash/runner/views/sessions.py::RunnerSessionPollEndpoint`  | upsert new fields            |
| Stall reconcile | `apps/api/pi_dash/runner/tasks.py::reconcile_stalled_runs` (new)        | new Celery task              |
| DB migration    | `apps/api/pi_dash/runner/migrations/00XX_runner_agent_observability.py` | additive columns on `Runner` |
| UI panel        | `apps/web/core/components/runners/RunnerAgentStatusPanel.tsx` (new)     | new React component          |
