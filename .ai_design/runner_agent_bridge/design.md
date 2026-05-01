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
  belongs in a follow-up; this design only adds the agent*pid that
  would \_enable* such a command, not the command itself.
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

The runner reports state to the cloud on **two** different HTTP
shapes today, and the design has to extend both. They are not the
same envelope — be explicit about which fields go where so
implementation isn't ambiguous.

#### 4.2.1 Session-open `AttachBody` (top-level)

`runner/src/cloud/http.rs:265-275`. Sent once per session-open
(POST `/runners/<rid>/sessions/`); fields are at the top level of
the JSON body and `apply_hello`
(`apps/api/pi_dash/runner/services/session_service.py:55`)
ingests them by calling `body.get("...")` directly:

```rust
pub struct AttachBody {
    // existing
    pub version: String,
    pub os: String,
    pub arch: String,
    pub status: String,
    pub in_flight_run: Option<Uuid>,
    pub project_slug: Option<String>,
    pub host_label: String,
    pub agent_versions: HashMap<String, String>,

    // NEW — all optional. Snapshot of the agent's state at the
    // moment the session opens. Helpful so the cloud can paint
    // the UI immediately on (re)connect, without waiting for the
    // first poll.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_summary: Option<String>,
}
```

#### 4.2.2 Poll-time `PollStatus` (nested under `"status"`)

`runner/src/cloud/http.rs:756-785`. Sent on every long-poll (POST
`/runners/<rid>/sessions/<sid>/poll`). The poll body is `{ "ack":
[...], "status": <PollStatus> }`, and the cloud extracts it as
`status_entry = body.get("status") or {}`
(`apps/api/pi_dash/runner/views/sessions.py:240-242`):

```rust
pub struct PollStatus {
    // existing
    pub status: String,
    pub in_flight_run: Option<Uuid>,
    pub ts: DateTime<Utc>,

    // NEW — same fields as AttachBody's new fields. Ingested on every
    // poll (default cadence 25s) so the cloud's view is at most one
    // poll-cycle behind reality.
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

The two shapes carry the **same set of new fields** for symmetry —
the cloud applies an identical projection function to either source.
What differs is the envelope nesting and the ingestion call site
(see §4.5.1).

Wire impact: ~150-250 bytes per request. At a 25s poll cadence and
~60s session-open cadence (current observed behaviour, see PR #94
follow-up), that is ~10 bytes/sec/runner — negligible.

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

Mutating helpers — and crucially a **reset** call invoked from
`set_current_run(Some(...))`. Because the cloud-side schema is
per-`AgentRun` (§4.5.1), the runner-side watch state must be reset
at the moment a new run starts; otherwise the first heartbeat of
the new run reports the previous run's `tokens` / `turn_count` /
`last_event_at`, which would re-introduce the inheritance bug we
just designed away on the cloud side:

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

    /// Called from `set_current_run(Some(_))` to wipe per-run state
    /// before the new run's first heartbeat. Mirrors the cloud's
    /// per-run row scoping. Lifecycle is reset to `Preparing`
    /// (the supervisor stamps it synchronously on Assign — §5e6b1c0
    /// in PR #94 — so this is the natural starting state).
    pub async fn reset_per_run_state(&self) {
        self.tx_last_event_at.send_replace(None);
        self.tx_agent_pid.send_replace(None);
        *self.inner.tokens.lock().await = TokenUsage::default();
        *self.inner.turn_count.lock().await = 0;
        self.tx_agent_lifecycle.send_replace(AgentLifecycle::Preparing);
    }
}
```

The existing `set_current_run(Some(...))` at
`runner/src/daemon/state.rs:115` becomes the single point where
`reset_per_run_state()` is invoked. `set_current_run(None)` (run
finished) leaves the snapshot in place so the final heartbeat can
still carry the terminal totals; the next `Some(...)` clears them.

Call sites for the cheap signals (lifecycle, PID, last-event
timestamp, approvals) — these don't depend on agent-internal
protocol parsing:

- `supervisor.rs::pump_events` — call `note_agent_event(now)` on every
  `BridgeEvent` received (`Raw`, `RunStarted`, `ApprovalRequest`,
  etc. — every variant counts as activity).
- `agent::Bridge::spawn_*` — return the child PID through a new
  accessor (e.g. `Bridge::child_pid() -> Option<u32>`); supervisor
  calls `set_agent_pid(Some(pid))` after spawn,
  `set_agent_pid(None)` on shutdown.
- `pump_events` — flip lifecycle on the `BridgeEvent` variants it
  already matches: `RunStarted` → `Streaming`, `ApprovalRequest` →
  `AwaitingApproval`, `AwaitingReauth` → `AwaitingReauth`,
  `Completed` → `Completing`, `Failed` / stdout-close → `Dead`.
- A new tokio interval task in the supervisor checks
  `now - last_event_at` periodically and flips the lifecycle to
  `Thinking` / `Stalled` accordingly.

#### 4.3.1 Token / turn extraction — bridge enrichment, not direct mutation

The token count and turn-boundary signals do **not** exist as
discrete `BridgeEvent` variants today (see §1). To surface them
without coupling the supervisor to agent-protocol details, this
design adds new variants to `BridgeEvent`:

```rust
pub enum BridgeEvent {
    // ... existing variants ...

    /// Cumulative token usage update from the agent. Codex emits this
    /// on its `codex/event/token_count` frames; the bridge layer
    /// promotes them out of `Raw`. Claude does not stream these — it
    /// only reports usage in the terminal `Result` payload, so the
    /// Claude bridge emits a single `TokenUpdate` immediately before
    /// `Completed` (degraded fidelity, but avoids missing the totals).
    TokenUpdate {
        run_id: Uuid,
        usage: TokenUsage,
    },
    /// Turn boundary marker. Codex: `turn/started` (or a fresh
    /// `session_started.thread_id`). Claude: not surfaced (Claude is
    /// not turn-structured the way Codex is) — `turn_count` stays at
    /// 1 for Claude runs.
    TurnBoundary {
        run_id: Uuid,
    },
}
```

Call sites:

- `runner/src/codex/bridge.rs::BridgeCursor::translate` — when the
  inbound notification's method matches `codex/event/token_count`,
  emit `TokenUpdate` instead of (or in addition to) `Raw`. When
  method matches `turn/started`, emit `TurnBoundary`.
- `runner/src/claude_code/bridge.rs::translate` — on `StreamEvent::
Result`, parse usage from the result and emit a final
  `TokenUpdate` before `Completed`. No `TurnBoundary` for Claude.
- `supervisor.rs::pump_events` — match the new variants, call
  `state.add_tokens(...)` and `state.incr_turn()`.

Why this shape rather than supervisor-side `Raw.params` parsing:
the supervisor is meant to be agent-agnostic. Putting protocol
knowledge there ("codex emits token_count frames with this shape")
duplicates what the bridge layer is for. Enriching `BridgeEvent`
with two new variants keeps the layering intact and makes the
Claude-degraded story explicit in the bridge code rather than
hidden in the supervisor.

#### 4.3.2 Claude-agent degraded story (explicit)

Because Claude does not stream token-usage or turn-boundary frames,
the heartbeat fidelity is lower for Claude runs than for Codex runs:

| Field                | Codex                       | Claude                                       |
| -------------------- | --------------------------- | -------------------------------------------- |
| `agent_pid`          | live                        | live                                         |
| `agent_state`        | live                        | live (state machine driven by `Raw` cadence) |
| `last_event_at`      | bumped per frame            | bumped per frame                             |
| `tokens`             | streamed via `TokenUpdate`  | one final `TokenUpdate` at end of run        |
| `turn_count`         | streamed via `TurnBoundary` | always 1                                     |
| `last_event_summary` | populated from `Raw.method` | populated from `Raw.method`                  |
| `approvals_pending`  | live                        | live                                         |

This is acceptable — the most operationally important signals
(`agent_state`, `last_event_at`, `agent_pid`, `approvals_pending`)
work uniformly across both agents. The cosmetic ones (token totals
during a run, turn count) are Codex-only by design. The web UI
should render `tokens=null` for Claude runs as "—" rather than
"0", to avoid suggesting zero token usage on a real run.

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

#### 4.5.1 Persist the heartbeat fields — on `AgentRun`, not `Runner`

Symphony's per-running-entry record lives on the running issue, not
on the worker. Pi-dash should match: the observability fields are
**per-run**, not per-runner. Storing them on `Runner` would let a
finished run's `last_event_at` / `tokens` / `turn_count` carry over
into a brand-new run on the same worker before the next heartbeat
arrives, and the watchdog could fail the new run for a stall it
inherited from the previous one. Per-run scoping makes that class
of bug unrepresentable.

So the new columns go on `AgentRun`. All nullable, so an old runner
(or a brand-new run that hasn't received its first heartbeat yet)
shows as "unknown" rather than the misleading "idle, 0 tokens":

```python
# apps/api/pi_dash/runner/models.py — AgentRun extensions
class AgentRun(BaseModel):
    # ... existing fields ...

    agent_lifecycle      = models.CharField(max_length=32, null=True, blank=True)
    agent_pid            = models.IntegerField(null=True, blank=True)
    last_event_at        = models.DateTimeField(null=True, blank=True)
    agent_turn_count     = models.PositiveIntegerField(null=True, blank=True)
    agent_input_tokens   = models.BigIntegerField(null=True, blank=True)
    agent_output_tokens  = models.BigIntegerField(null=True, blank=True)
    agent_total_tokens   = models.BigIntegerField(null=True, blank=True)
    agent_approvals_pending = models.PositiveSmallIntegerField(null=True, blank=True)
    agent_last_event_summary = models.CharField(max_length=200, null=True, blank=True)

    class Meta:
        # ... existing ...
        indexes = [
            # ... existing ...
            # supports the stall reconcile query in §4.5.2.
            models.Index(fields=["status", "last_event_at"]),
        ]
```

`NULL` is the canonical "unknown" / "pre-observability" sentinel —
the watchdog (§4.5.2) excludes NULL via Django's standard `__lt`
semantics, the UI renders NULL as "—", and the migration is purely
additive (no backfill).

Ingestion has **two distinct paths** to match the wire shapes
(§4.2):

**Session-open** (`apply_hello`,
`apps/api/pi_dash/runner/services/session_service.py:55`). The body
is the top-level `AttachBody`. Identify the active run for this
runner by querying `AgentRun` where `runner=runner` and
`status__in=BUSY_STATUSES`, then upsert the new fields on that row.
If `body["in_flight_run"]` is present, prefer that as the run
selector to avoid ambiguity. If no active run exists, skip the
upsert entirely (the runner is idle; no row to update).

**Poll** (`RunnerSessionPollEndpoint`,
`apps/api/pi_dash/runner/views/sessions.py:240`). The body is
`{"ack": ..., "status": status_entry}`. Read `status_entry`, then
do the same lookup and upsert as the session-open path. The shared
projection function:

```python
# apps/api/pi_dash/runner/services/session_service.py (new helper)
def apply_agent_state(runner, status_entry, run_id_hint=None):
    """Upsert agent observability fields on the runner's active run.

    `status_entry` may be the top-level AttachBody (session-open) or
    the nested PollStatus (poll-time) — both carry the same set of
    fields. Missing fields are left as-is on the existing row (i.e.
    we never NULL out a known good value because of a stale poll).
    """
    run = _resolve_active_run(runner, run_id_hint)
    if run is None:
        return
    # Apply only the fields that are present in this heartbeat.
    update_fields = []
    for src_key, model_field in AGENT_STATE_FIELD_MAP.items():
        if src_key in status_entry:
            setattr(run, model_field, status_entry[src_key])
            update_fields.append(model_field)
    if update_fields:
        run.save(update_fields=update_fields)
```

Both `apply_hello` and the poll handler call `apply_agent_state`
with the same projection, but extract the source dict differently
(top-level vs nested). The runner's `in_flight_run` field — already
present in both shapes — is the run-id hint that disambiguates
when the runner has multiple recent BUSY runs from quick-succession
assignments.

#### 4.5.2 Server-side stall watchdog

A Celery beat task running every 30s — Symphony's
`reconcile_stalled_running_issues` analogue:

```python
# apps/api/pi_dash/runner/tasks.py
@app.task
def reconcile_stalled_runs():
    threshold = settings.RUNNER_AGENT_STALL_THRESHOLD_SECS  # default 300
    cutoff = timezone.now() - timedelta(seconds=threshold)

    # Filters directly on AgentRun.last_event_at (per-run, §4.5.1).
    # NULL last_event_at — old runners or runs that have never
    # received their first heartbeat — is excluded by Django's __lt
    # semantics, so old fleets remain compatible without special
    # casing.
    stalled_runs = AgentRun.objects.filter(
        status__in=BUSY_STATUSES,
        last_event_at__lt=cutoff,
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
overrideable per-run via the agent kind in the run's runner config
for deployments with intentionally slow agents.

#### 4.5.3 Web UI surface

`apps/web/core/components/runners/...` gains a per-run detail panel
(scoped to the active `AgentRun`, not to the worker) reading from
the new fields. Every NULL value renders as "—" or "unknown",
never as a hard zero — that's the difference from the previous
draft and the reason the schema (§4.5.1) is nullable end-to-end.

| UI element        | Source field (on `AgentRun`)    | NULL render | Format                                                                          |
| ----------------- | ------------------------------- | ----------- | ------------------------------------------------------------------------------- |
| Agent state badge | `agent_lifecycle`               | "unknown"   | colored chip (green=streaming, yellow=thinking, red=stalled/dead, gray=unknown) |
| Last activity     | `last_event_at`                 | "—"         | "12s ago", "2m ago"                                                             |
| Agent PID         | `agent_pid`                     | "—"         | numeric, with copy button                                                       |
| Turn              | `agent_turn_count` / config max | "—"         | "2 / 5"                                                                         |
| Tokens            | `agent_total_tokens`            | "—"         | "31.0k"                                                                         |
| Approvals         | `agent_approvals_pending`       | "0"         | numeric, with link to approvals view                                            |
| Last event        | `agent_last_event_summary`      | hidden      | tooltip                                                                         |

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

B.1 DB migration: nullable columns on `AgentRun` (per-run, see
§4.5.1) plus a `(status, last_event_at)` index for the watchdog.
B.2 Shared `apply_agent_state(runner, status_entry, run_id_hint)`
helper. `apply_hello` extracts the top-level fields from
`AttachBody`; the poll handler extracts `body["status"]`. Both call
the same helper.
B.3 Server-side stall reconcile Celery task (queries `AgentRun`
directly; NULL `last_event_at` is auto-skipped by `__lt`).
B.4 Tests covering: backward compat (old heartbeat shape, NULL
fields stay NULL), stall detection true positive / false positive,
awaiting-approval suppression, run-id-hint disambiguation when
multiple BUSY runs exist for one runner.

When B ships, runners can flip the flag on; old runners continue to
work without the new fields. Critically, an old run that completes
on a now-upgraded fleet does **not** poison the next run's metrics
because the fields are scoped to `AgentRun`, not `Runner`.

### Phase C — Operator surface

C.1 Web UI runner detail panel.
C.2 Optional JSON observability endpoint
(`GET /api/v1/runners/<rid>/agent-status`) for external dashboards.
C.3 Status badges in the runs list view (color of last activity).

## 6. Backward and Forward Compatibility

The `AgentRun` columns are nullable end-to-end; `NULL` is the
canonical "unknown" sentinel and is what the watchdog and UI both
fall back to. There is no `default="idle"` or `default=0` in the
schema — those would silently misrepresent old / pre-observability
runs as "idle, 0 tokens" instead of "unknown."

| Direction                        | Effect                                                                                                                                                               |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| New runner → old cloud           | Old cloud ignores unknown fields on both `AttachBody` and `PollStatus`; reaper still works on the existing `in_flight_run`.                                          |
| Old runner → new cloud           | Heartbeat carries no new fields; columns remain `NULL`; UI renders "—"; stall reconcile skips the run because `__lt` excludes `NULL`.                                |
| New runner → new cloud, flag off | Heartbeat carries no new fields; behaves identically to the previous row.                                                                                            |
| Mixed fleet during rollout       | Each `AgentRun` row is independent; no cross-runner state comparison. A new run on the same runner starts with all `NULL`s and is filled by its own first heartbeat. |

Field stability:

- `AgentLifecycle` is serialized as snake_case strings; new variants
  may be added; the cloud parses with a default fallback to
  `"unknown"` so it never crashes on a future variant.
- `tokens` / `turn_count` are scoped to one `AgentRun` row; resetting
  is automatic because each new run gets a fresh row. There is no
  cross-run carry-over to manage.
- Heartbeats may omit fields (e.g. `tokens` for Claude during a run);
  ingestion preserves the previous non-NULL value rather than
  blanking it. See `apply_agent_state`'s "only the fields that are
  present" semantic in §4.5.1.

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

| Component                | File                                                                      | Change                                                                            |
| ------------------------ | ------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Lifecycle enum           | `runner/src/agent/lifecycle.rs` (new)                                     | new module                                                                        |
| State store              | `runner/src/daemon/state.rs`                                              | add per-run fields + setters; reset all on each `set_current_run(Some)`           |
| `BridgeEvent` enrichment | `runner/src/agent/mod.rs`                                                 | add `TokenUpdate` and `TurnBoundary` variants                                     |
| Codex translator         | `runner/src/codex/bridge.rs::BridgeCursor::translate`                     | promote `codex/event/token_count` and `turn/started` out of `Raw`                 |
| Claude translator        | `runner/src/claude_code/bridge.rs::translate`                             | emit one `TokenUpdate` from the terminal `Result` payload                         |
| Event ingestion          | `runner/src/daemon/supervisor.rs::pump_events`                            | call `note_agent_event`; handle new variants; drive lifecycle                     |
| Bridge PID               | `runner/src/agent/mod.rs::AgentBridge`                                    | expose `child_pid() -> Option<u32>`                                               |
| AttachBody (open)        | `runner/src/cloud/http.rs::AttachBody` (line 265)                         | add new optional **top-level** fields                                             |
| PollStatus (poll)        | `runner/src/cloud/http.rs::PollStatus` (line 757)                         | add new optional fields (nested under `"status"` in poll body)                    |
| Hello apply              | `apps/api/pi_dash/runner/services/session_service.py::apply_hello`        | call new `apply_agent_state(runner, body, run_id_hint=body.get("in_flight_run"))` |
| Poll handler             | `apps/api/pi_dash/runner/views/sessions.py::RunnerSessionPollEndpoint`    | call `apply_agent_state(runner, body.get("status") or {}, run_id_hint=...)`       |
| Stall reconcile          | `apps/api/pi_dash/runner/tasks.py::reconcile_stalled_runs` (new)          | new Celery task; queries `AgentRun.last_event_at` directly                        |
| DB migration             | `apps/api/pi_dash/runner/migrations/00XX_agentrun_agent_observability.py` | additive **nullable** columns on `AgentRun` + `(status, last_event_at)` index     |
| UI panel                 | `apps/web/core/components/runners/RunnerAgentStatusPanel.tsx` (new)       | new React component; renders NULL as "—" / "unknown"                              |
