# Issue Run Continuation — Multi-Run Conversations Per Issue

> Directory: `.ai_design/issue_run_improve/`
>
> **Status:** the comment-auto-trigger sections (notably §5.2 and the
> terminate-side comment sweep) are **superseded** by
> `.ai_design/issue_ticking_system/design.md`, which replaces
> automatic comment-triggered runs with a periodic scheduler + an
> explicit "Comment & Run" UI button. The PAUSED / pinning /
> native-resume / drain machinery in this doc carries over unchanged
> — only the trigger model changes. Read both docs together.
>
> **Scope:** how an issue continues across more than one `AgentRun` when the
> first run yields without finishing — and what that implies for run lifecycle,
> dispatch routing, and conversation continuity.
>
> **Related work**
>
> - `.ai_design/issue_runner/design.md` — pods, runners, the queue model, the
>   run-identity split (creator vs. billable owner). This doc builds on top of
>   that one and assumes its decisions.
> - `.ai_design/issue_ticking_system/design.md` — periodic re-invocation of
>   the agent, replacing this doc's comment-auto-trigger model.

## 1. Problem

Today, on `main` (commit `a99e736`):

- An issue moved to **In Progress** auto-creates an `AgentRun` and dispatches
  it to a runner via the pod queue (`orchestration/signals.py`,
  `orchestration/service.py`, `runner/services/matcher.py`).
- The runner spawns a fresh `claude --print …` (or codex equivalent) for every
  run. Each run is a brand-new agent session.
- When that run terminates — for any reason — the agent's working memory is
  gone. There is no resume path consumed by the cloud, no follow-up trigger
  on user comments, no notion of a "paused" run.
- A user comment on an issue is **not** a signal: nothing wakes the agent.

The data model already partially supports continuation:

- `AgentRun.thread_id` (`runner/models.py:363`) stores the provider's session
  id (Claude `session_id` / Codex `thread_id`) when the runner reports it via
  `run_started` (`consumers.py:374-379`).
- `AgentRun.parent_run` (`runner/models.py:346-352`) chains follow-up runs.
  `orchestration/service.py:103` sets it to the latest prior run when a new
  run is created for an issue that already has one.
- The runner `RunPayload` struct has a `resume_thread_id: Option<String>` field
  (`runner/src/agent/mod.rs:54`) and the **Codex bridge already consumes it**
  (`runner/src/codex/bridge.rs:61-72`) — calling `thread/resume` when set,
  `thread/start` otherwise. The Claude Code path explicitly skips resume
  (`runner/src/claude_code/mod.rs:10`: "Single-turn runs only: no `--resume`").
- The `Assign` wire envelope (`runner/src/cloud/protocol.rs:109-123`) does not
  carry `resume_thread_id`, and the supervisor hardcodes `resume_thread_id:
None` at `runner/src/daemon/supervisor.rs:544` regardless.

So everything is wired _for storage_ and partially _for runner consumption_,
but nothing on the cloud side ever populates the resume hint. This doc
proposes how to wire the consumption side, and the dispatch model that lets
it work without sacrificing throughput.

## 2. Goal

For an issue whose work spans multiple agent runs, give the agent enough
context on each follow-up run that it can continue meaningfully — while
keeping the agent it ran on, when possible, so native session resume gives us
correctness for free.

The shape we want:

```
T0: issue001 dispatched to agentA → runs → yields → PAUSED
T1: agentA picks issue002 from pod queue → BUSY on issue002
T2: human comments on issue001 → run001b created, pinned to agentA → QUEUED
T3: agentA finishes issue002 → idle → picks run001b from its personal queue
T4: agentA resumes issue001's session natively (claude --resume / thread/resume)
```

The key property: agentA is never blocked. While issue001 is paused waiting
for human input (potentially hours), agentA serves other issues from the pod
queue. When the human finally responds, run001b is waiting in agentA's
personal queue and gets picked up the moment agentA frees up.

Non-goals:

- Multi-agent collaboration on the same issue (one conversation per chain).
- Cross-issue memory (a runner's recall of issue A is not available on
  issue B; that's a separate, larger problem).
- Long-term memory beyond a single issue's lifecycle.

## 3. The three sub-problems

| #   | Question                                                      | Section                      |
| --- | ------------------------------------------------------------- | ---------------------------- |
| A   | What does it mean for a run to "yield" instead of "finish"?   | §4 — Run Lifecycle           |
| B   | What triggers a follow-up run, and where does it dispatch to? | §5 — Continuation & Dispatch |
| C   | How does the follow-up run resume the prior run's context?    | §6 — Conversation Continuity |

## 4. Run Lifecycle: Yield vs. Finish

### 4.1 Today's status surface

`AgentRunStatus` (`apps/api/pi_dash/runner/models.py:101-110`) actually has:
`QUEUED, ASSIGNED, RUNNING, AWAITING_APPROVAL, AWAITING_REAUTH, BLOCKED,
COMPLETED, FAILED, CANCELLED`. Not the simpler enum sketched in earlier
drafts — `BLOCKED` is already present and load-bearing.

And there are **multiple independent classifiers**, not just two:

| Classifier                                         | Where                           | Members today                                                 |
| -------------------------------------------------- | ------------------------------- | ------------------------------------------------------------- |
| `BUSY_STATUSES` (matcher excludes these runners)   | `runner/services/matcher.py:51` | ASSIGNED, RUNNING, AWAITING_APPROVAL, AWAITING_REAUTH         |
| `NON_TERMINAL_STATUSES` (gates pod deletion)       | `runner/services/matcher.py:42` | QUEUED + the four BUSY                                        |
| `AgentRun.is_terminal`                             | `runner/models.py:403-410`      | COMPLETED, FAILED, CANCELLED, **BLOCKED**                     |
| `AgentRun.is_active` (single-active-run guardrail) | `runner/models.py:412-421`      | QUEUED, ASSIGNED, RUNNING, AWAITING_APPROVAL, AWAITING_REAUTH |
| `ACTIVE_RUN_STATUSES` (Prom metrics)               | `runner/views/metrics.py:39`    | ASSIGNED, RUNNING, AWAITING_APPROVAL, AWAITING_REAUTH         |

Any new status has to be placed in **all five**. Treating `BUSY_STATUSES`
alone is incomplete — `is_active` in particular is what `_active_run_for`
(`orchestration/service.py:45`) consults to enforce the single-active-run
guardrail per issue, which directly affects whether comment-triggered
continuation can create a follow-up.

### 4.2 Reconciling with the existing `BLOCKED` contract

`BLOCKED` already terminates a run when the agent emits the
`pi-dash-done` fenced block with `status: "blocked"`
(`apps/api/pi_dash/orchestration/done_signal.py:32, 154-159`). The agent
populates `autonomy.question_for_human` and `blockers[]` in the same
payload. So today, "agent has a question" is already representable — but it
goes to **terminal** BLOCKED, with no resume path.

We have two options:

**Option A — `"blocked"` becomes resumable.** Change `done_signal.py:154-159`
so the agent's `"blocked"` status maps to `PAUSED_AWAITING_INPUT` (the new
status) when `autonomy.question_for_human` is set, and to terminal `BLOCKED`
otherwise. One agent contract, two cloud outcomes.

- _Pro:_ no agent-side change; existing prompts continue to work.
- _Con:_ the "BLOCKED is terminal" invariant some code already assumes
  becomes "BLOCKED is sometimes terminal" — must audit callers.

**Option B — add a distinct `"paused"` agent status.** Extend
`VALID_STATUSES = {"completed", "blocked", "noop", "paused"}`; agent emits
`"paused"` when it can resume on reply, `"blocked"` when it has genuinely
given up. Cloud maps `"paused"` → `PAUSED_AWAITING_INPUT`.

- _Pro:_ preserves the BLOCKED-is-terminal invariant; clearer agent intent.
- _Con:_ requires updating the agent system prompt and any prompts that
  document the done-signal schema; coordination across all agent paths.

**Recommendation: Option B.** The audit cost in Option A is unbounded
(every code path reading BLOCKED would need re-checking), and the prompt-
side change is the smaller surface. We update the system prompt's done-
signal schema once and the runtime contract stays clean.

### 4.3 New status: `PAUSED_AWAITING_INPUT`

Add `PAUSED_AWAITING_INPUT` to `AgentRunStatus` and place it in the
classifiers as follows:

| Classifier              | PAUSED_AWAITING_INPUT in? | Why                                                                               |
| ----------------------- | ------------------------- | --------------------------------------------------------------------------------- |
| `BUSY_STATUSES`         | **No**                    | Runner is free to take other pod work while paused (§5.3 interleaving)            |
| `NON_TERMINAL_STATUSES` | Yes                       | Pod can't be deleted while a paused run exists (must drain first)                 |
| `is_terminal`           | No                        | Run can resume; it isn't "done"                                                   |
| `is_active`             | **No**                    | Single-active-run guardrail must allow a fresh follow-up to be created (see §5.2) |
| `ACTIVE_RUN_STATUSES`   | No                        | Operational metric tracks runs occupying runner capacity; paused doesn't          |

The `is_active` exclusion is the subtle one. Today `_active_run_for`
prevents creating a new run when one already exists in `is_active` for the
same issue. If PAUSED_AWAITING_INPUT were in `is_active`, the comment
trigger could never create a follow-up — the paused run would block its own
successor. So PAUSED is explicitly outside `is_active`, and the comment-
trigger path additionally coalesces (§5.7) to avoid creating two parallel
followers.

(One subtle implication of PAUSED ∈ `NON_TERMINAL_STATUSES` but PAUSED ∉
`BUSY_STATUSES`: the matcher won't avoid a pod with paused runs, but the
pod-deletion path will refuse to drop the pod until those paused runs are
drained. That's the intended behavior — paused runs hold cloud-side state
that we want to preserve until explicitly resolved.)

### 4.4 The "I'm yielding" protocol

For `PAUSED_AWAITING_INPUT` to exist, the agent needs a clean way to emit a
yield. The mechanism diverges per agent because today's plumbing is
materially different:

**Codex.** Approvals are wired (`runner/src/codex/bridge.rs` accepts
`ApprovalResponseParams`; the JSON-RPC tool surface is live), and
`thread/resume` works end-to-end. Add a `request_user_input` tool. The
runner sees the tool call, sends `ClientMsg::RunPaused { ... }` to cloud,
and lets the Codex thread idle on disk for later resume. This is the
"piggyback the approval path" plan — and it works _for Codex_.

**Claude Code.** Two limitations block the same approach:
`runner/src/claude_code/mod.rs:6-10` documents that approvals are bypassed
(`--permission-mode bypassPermissions`) and that resume is not wired.
`runner/src/claude_code/bridge.rs:122-128` explicitly fails fast if an
approval is routed in, because the MCP permission-prompt bridge isn't
built. There is no tool-call surface to piggyback on.

For Claude, the pragmatic path reuses the existing `pi-dash-done` channel:

1. Extend `done_signal.py:VALID_STATUSES` to include `"paused"` (Option B
   above). Add an optional `next_steps_if_resumed` field to the payload
   schema.
2. The system prompt instructs Claude to emit `status: "paused"` (instead
   of `"blocked"`) when it has a question and can resume.
3. Cloud's `done_signal.ingest_into_run` maps `"paused"` →
   `PAUSED_AWAITING_INPUT` and stores the payload on `done_payload`.
4. No runner-side bridge change; the runner already forwards Claude's
   final stdout, and `done_signal.py` already parses it.
5. **Resume-side** for Claude still requires the `--resume` plumbing
   (§7.4 / §8.2 item 2). The yield path and the resume path are
   independent changes; both must land for Claude continuation to work.

**Heuristic on output** (sniffing for "needs input" markers) and
**fully-untyped prompt protocol** are still rejected — too brittle. The
above is two structured paths, one per agent, both grounded in existing
plumbing.

**Codex yield path** (tool call):

- Runner observes the `request_user_input` tool call from Codex.
- Runner sends `ClientMsg::RunPaused { run_id, payload, paused_at }` to
  cloud (new variant — see §7.2).
- Cloud's `on_run_paused` consumer handler transitions the run to
  `PAUSED_AWAITING_INPUT`, persists the payload to `done_payload`, and
  triggers `drain_for_runner` (§5.6).
- Runner ack's the tool call to Codex (so the thread parks cleanly on
  disk for later resume) and returns to idle.

**Claude yield path** (done-signal fenced block):

- Claude emits its terminal `pi-dash-done` fence with `status: "paused"`
  and a `done_payload` containing the question + handoff fields.
- Runner forwards Claude's terminal output verbatim (no bridge change)
  and reports run completion via the existing `RunCompleted` /
  done-signal flow.
- Cloud's `done_signal.ingest_into_run` parses the fence, sees
  `"paused"`, transitions the run to `PAUSED_AWAITING_INPUT` instead of
  terminal `BLOCKED`, and triggers `drain_for_runner`.
- No new `ClientMsg` variant is needed for Claude.

Both paths converge on `PAUSED_AWAITING_INPUT` with a populated
`done_payload`. The payload schema (§6.4) is shared — question for the
human plus enough state for a fresh agent to continue if native resume
fails.

## 5. Continuation Triggers and Dispatch

### 5.1 Candidate triggers

| Trigger                                    | Carries new info? | UX feel            | Implementation cost                 |
| ------------------------------------------ | ----------------- | ------------------ | ----------------------------------- |
| User comment on the issue                  | Yes               | Conversational     | New `IssueComment` post_save signal |
| Issue state transition back to In Progress | No                | Coarse, button-y   | Already wired                       |
| Explicit "Continue" / "Reply" action       | Optional          | Explicit, button-y | New endpoint + UI                   |
| Approval grant (resumes paused run)        | No                | Invisible          | Already partially wired             |

**Recommendation**: comment-triggered continuation as the primary path
(matches the user's mental model: "I leave a comment, the agent reads it"),
with explicit "Continue" as a secondary path for cases where the user has
nothing to add but wants the agent to keep going.

### 5.2 Comment-triggered flow

Today, run creation is triggered exclusively by issue state transitions
(`apps/api/pi_dash/orchestration/signals.py:48` →
`handle_issue_state_transition`). Comments are inert. This proposal adds a
second trigger via a `post_save(IssueComment)` receiver.

```
User adds comment to Issue I
        │
        ▼
post_save(IssueComment)
        │
        ├─► Skip if comment.actor.is_bot (excludes pi_dash_agent workpad bot — see workpad.py)
        ├─► Skip if no prior AgentRun for I (no conversation to continue)
        ├─► Skip if issue.state.group ∈ frozen-states (see gating below)
        ├─► Coalesce: if a QUEUED follow-up already exists for I, append to its prompt and stop
        ├─► Find latest AgentRun R_prev for I
        │     │
        │     ├─► R_prev is PAUSED_AWAITING_INPUT → create R_next, pin to R_prev.runner
        │     ├─► R_prev is is_terminal           → create R_next, pin to R_prev.runner
        │     └─► R_prev is is_active             → no-op here; terminate-side
        │                                            sweep picks it up (see below)
        │
        ▼
R_next dispatched through normal pod queue (with personal-queue priority)
```

**Comments-during-active-run.** When R_prev is still `is_active`
(RUNNING / ASSIGNED / AWAITING_APPROVAL / AWAITING_REAUTH), the
`post_save` receiver does **not** create R_next, and the comment is
**not** held in any side table or queue. Instead, the comment lives
exactly where every comment lives — in `IssueComment` — and is
discovered later by a **timestamp sweep** in the terminate handlers:

```
on_run_completed / on_run_failed / on_run_paused (consumers.py)
        │
        │ (existing run-state update + drain trigger)
        ▼
maybe_continue_after_terminate(R):
    if R.work_item_id is None: return
    pending = IssueComment.objects.filter(
        issue=R.work_item,
        actor__is_bot=False,
        created_at__gt=R.started_at,
    ).exists()
    if not pending: return
    invoke comment-trigger flow on R.work_item
        (which reads the same comments to compose R_next.prompt
         and applies the same gating + pinning rules above)
```

This means there is no "pending comment" model field, no separate
queue, no new persistence. The storage is `IssueComment` itself; the
"queue" is implicit via the timestamp range
`created_at > R_prev.started_at`. The prompt builder for R_next reads
all unconsumed comments in that range and composes them into the
prompt — naturally coalescing multiple comments arriving during the
active run.

(One subtle detail: the sweep filter must use `R.started_at` rather
than `R.assigned_at` so comments left between assignment and the
agent actually starting work are included. Also worth scoping by
`R.created_by` or excluding the trigger comment itself depending on
intended semantics — see Q9.)

**Bot filter.** The `pi_dash_agent` bot user
(`apps/api/pi_dash/orchestration/workpad.py:27-70`) authors the agent's
workpad comments with `is_bot=True`. The trigger MUST filter
`comment.actor.is_bot` — not a vague "skip runner/agent" — otherwise the
agent's own workpad updates would re-trigger continuation in a loop.

**State gating.** The comment trigger should respect the issue's lifecycle:

- If `issue.state.group ∈ {COMPLETED, CANCELLED}` (terminal groups): do not
  dispatch. The user should re-open the issue to resume work.
- If `issue.state.group ∈ {BACKLOG, UNSTARTED}`: do not dispatch. State-
  transition path owns starting work; the comment trigger only continues
  in-progress work.
- If `issue.state.group = STARTED` (default "In Progress"): dispatch.
- Custom workspace state groups: defer to the workspace's policy; for MVP
  treat anything not in the above set as "no dispatch" (conservative).

**Guardrail interaction.** The existing single-active-run guardrail in
`_active_run_for` (`orchestration/service.py:45-59`) blocks new runs when
one is already `is_active` for the issue. With PAUSED*AWAITING_INPUT
explicitly \_outside* `is_active` (§4.3), a paused run does not block the
follow-up — which is what we want. But two distinct comments arriving in
quick succession could both observe "no active run" and create two
QUEUED follow-ups for the same issue. The **coalesce step above** handles
this: before creating R_next, the trigger checks for an existing QUEUED
run with `parent_run = R_prev` (or the same chain) and appends the new
comment text to that run's prompt instead of creating a duplicate. This
keeps the chain linear.

### 5.3 Pinning model: strict pin with interleaving

When R_next is created from a comment-trigger, set
`pinned_runner_id = R_prev.runner_id`. This is **strict pin** — only the
original runner picks it up. Resume affinity is preserved and we get cheap,
high-fidelity native resume on every continuation.

Strict pin works without blocking the runner because of §4.2 — paused runs
don't make the runner busy, so the runner stays available for fresh pod work.
The pin just means "when that runner is next idle, this is what it takes."

This is the central design choice. Earlier sketches (soft pin with replay
fallback, no pin with replay always) gave up on native resume in the common
case. Strict-pin-with-interleaving keeps native resume as the primary
mechanism while still letting agentA do useful work during human latency.

### 5.4 Two queues, one table

Each runner now has, conceptually, two queues feeding it:

1. **Personal queue**: runs `pinned_runner_id = me`. Strict pin.
2. **Pod general queue**: runs with `pinned_runner_id IS NULL`.

But this is **one `AgentRun` table, two filters** — not a new table or a new
data structure. The matcher's per-runner query becomes:

```sql
SELECT FROM agent_run
WHERE pod = <runner.pod>
  AND status = QUEUED
  AND (pinned_runner_id = <runner.id> OR pinned_runner_id IS NULL)
ORDER BY (pinned_runner_id = <runner.id>) DESC, created_at ASC
LIMIT 1
FOR UPDATE SKIP LOCKED
```

Pinned-to-me runs come first; otherwise FIFO over unpinned. Pinned-to-someone-
else runs are excluded entirely.

### 5.5 Dispatch loop: runner-first

Today's `drain_pod` is run-first — pick a QUEUED run, then a runner. With
pinning, runner-first is cleaner (avoids head-of-line blocking when the head
QUEUED run is pinned to a busy runner):

```python
def drain_pod(pod):
    assignments = []
    with transaction.atomic():
        for runner in idle_runners_in_pod_locked(pod):
            run = next_for_runner_locked(runner)  # the SQL above
            if run is None:
                continue
            assign(run, runner)
            assignments.append((runner, run))
    for runner, run in assignments:
        on_commit(lambda: send_to_runner(runner.id, build_assign_msg(run)))
```

`drain_for_runner(runner)` is `drain_pod` narrowed to one runner — same
query, same locking, same `on_commit` send.

### 5.6 Drain triggers

Drain is invoked at three moments:

1. **New run enqueued** (existing) — `transaction.on_commit(matcher.drain_pod_by_id(pod.id))` in `orchestration/service.py:195`. Already wired.
2. **Run terminates** (SUCCEEDED / FAILED / PAUSED) → runner becomes idle → `drain_for_runner(runner)`. **New**, lives in the Channels consumer where run state is updated. Without this, agentA never picks up its own pinned run after finishing issue002.
3. **Runner reconnects** → `drain_for_runner(runner)` in the consumer's connect handler. **New**, covers the case where pinned runs were enqueued while the runner was offline.

### 5.7 Edge cases

- **Two comments before R_next dispatches.** Coalesce: at run-creation time,
  if there's already a QUEUED R_next for the issue, append the new comment
  to its prompt instead of creating a second run.
- **Comment arrives while R_prev is RUNNING.** Don't queue a new run yet.
  Two future options: (a) push the comment to the live run as an inbound
  user message (out of MVP scope; needs runner-side support for mid-run
  inbound). For MVP, take (b) — the terminate-side timestamp sweep in
  §5.2 picks up any non-bot comments with `created_at > R.started_at`
  when the run terminates, and runs the comment-trigger logic. No
  separate "pending comment" storage is needed.
- **Agent comments on its own issue.** Filter by author type; agent-authored
  comments do not re-trigger.
- **Pinned runner offline / revoked.** The pinned run sits in QUEUED forever
  unless we intervene. Two mitigations:
  - On `Runner.status = REVOKED` transition, release pinned QUEUED runs:
    `UPDATE AgentRun SET pinned_runner=NULL WHERE pinned_runner=R.id AND status=QUEUED`. They fall back to the pod general queue.
  - **Operator escape hatch in UI**: a "release pin" button on the run row
    that sets `pinned_runner_id = NULL`. The MVP-correct answer is operator-
    driven, not a TTL — TTLs silently lose the resume benefit and are easy
    to misconfigure.
- **Multiple paused conversations on the same agent.** issue001, issue003 both
  paused on agentA, both get human comments. R001b and R003b both pin to
  agentA. When agentA finishes issue002, FIFO by `created_at` of the new run
  is the obvious order — earliest comment wins.
- **No thread_id to resume against.** If `R_prev.thread_id IS NULL` (parent
  crashed before reporting one), pinning buys nothing — there's no session
  to resume. Skip the pin; the new run goes to the pod general queue and any
  runner takes it.

## 6. Conversation Continuity

### 6.1 Primary mechanism: native session resume

When R_next is dispatched:

- Cloud sets `resume_thread_id = R_prev.thread_id` on the `Assign` envelope.
- Runner passes it through to the agent CLI:
  - **Codex**: already wired — `runner/src/codex/bridge.rs:61-72` calls
    `thread/resume` when `resume_thread_id` is set.
  - **Claude Code**: not yet wired — `runner/src/claude_code/mod.rs:10`
    explicitly says "single-turn only." Needs `--resume <session_id>`
    plumbing.

The agent CLI looks up its own on-disk session and reattaches:

- Claude Code: `~/.claude/projects/<workspace>/sessions/<session_id>.jsonl`
- Codex: its own data directory, keyed by thread id

### 6.2 The runner is stateless about past runs

The runner does **not** maintain any (issue → session_id) index. The cloud is
authoritative. The runner is a pass-through:

- `RunsIndex` (`runner/src/history/index.rs`) stores recents for the TUI but
  has no `thread_id` field.
- `HistoryWriter` (`runner/src/history/jsonl.rs`) writes per-run JSONL but
  doesn't index by session.
- `StateHandle` (`runner/src/daemon/state.rs`) is in-memory only; the
  `current_run.thread_id` field is wiped on terminate.

No new runner-side persistence is required for resume to work. The runner
just receives `Assign { resume_thread_id }` and passes the id to the agent
CLI's resume function. The agent CLI's own on-disk session store is the
artifact that matters.

(Optional observability: add `thread_id` to `RunSummary` in `RunsIndex` so
the TUI's Runs view can show "issue X, last session Y, on disk: ✓/✗" for
operator sanity-checks. Not on the dispatch critical path.)

### 6.3 What if native resume fails?

Resume can fail for legitimate reasons: agent CLI was reinstalled, session
file was manually deleted, host disk was wiped, the agent CLI's session
store rotated. The runner detects this when the agent CLI returns an error
on the resume call.

The runner reports `RunFailed { reason: ResumeUnavailable, ... }` to cloud.
Cloud's options:

1. **Re-dispatch as fresh-context** (recommended default). Drop the pin,
   drop the `resume_thread_id`, re-queue R_next. Whichever runner picks it
   up starts a fresh session, with the issue body + comments + the prior
   handoff (§6.4) as its context.
2. **Mark the issue as needing human attention.** Surface in UI: "the agent's
   prior session is no longer available; reply to start over." Right answer
   when the chain is so long that fresh-context-from-issue would be lossy
   enough to be misleading.

Option 1 is the default. Option 2 is an upgrade we may want later.

### 6.4 Agent-authored handoffs (the "free" fallback)

The yield tool (§4.3) requires a structured handoff payload, e.g.:

```json
{
  "question": "Should I keep the legacy export endpoint or remove it?",
  "summary_so_far": "Migrated /export/v1 to /export/v2; legacy still mounted; tests passing on v2.",
  "next_steps_if_resumed": "Either remove legacy mount in api/urls.py, or add a deprecation header."
}
```

The cloud stores this in `R_prev.done_payload` (or a dedicated field) and
posts it as an issue comment so it's human-visible. When fallback fresh-
context-from-issue happens (§6.3 option 1), the new agent reads the handoff
like any other comment and picks up.

This means even when native resume fails, a well-disciplined yield produces
enough state for a fresh agent to continue meaningfully — without any
transcript-replay machinery.

### 6.5 Why we're not building a transcript-replay path

An earlier sketch proposed reconstructing prior turns from `AgentRunEvent`
into a system-prompt preamble and starting a fresh provider session on every
follow-up. We're explicitly **not** building this for v1, because:

- Native resume covers the common case (same runner, same agent CLI, session
  on disk). It's cheaper (no token replay), higher-fidelity (real session
  state), and trivially correct.
- For the uncommon case where native resume fails, the issue body + comments
  - agent-authored handoff (§6.4) + branch state are already a durable,
    human-readable handoff. Replay competes with this for no clear win.
- Replay quality is sensitive to summarization. A poor preamble produces a
  worse continuation than just letting a fresh agent re-read the issue.

If observation later shows that fresh-context-from-issue is dropping too much
fidelity, replay can be added as a second fallback layer. It's not on the v1
critical path.

## 7. Wire Protocol Changes

### 7.1 `Assign` envelope (`runner/src/cloud/protocol.rs:109-123`)

Add one optional field:

```rust
Assign {
    run_id: Uuid,
    work_item_id: Option<Uuid>,
    prompt: String,
    repo_url: Option<String>,
    repo_ref: Option<String>,
    git_work_branch: Option<String>,
    expected_codex_model: Option<String>,
    approval_policy_overrides: Option<BTreeMap<String, serde_json::Value>>,
    deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    resume_thread_id: Option<String>,  // NEW
}
```

`#[serde(default)]` keeps it backward-compatible with older runners — a
field-only addition, so `protocol_version` does not need to be bumped.

### 7.2 Runner-to-cloud yield message (Codex only)

Add a new variant to `ClientMsg` for the Codex tool-call yield path:

```rust
RunPaused {
    run_id: Uuid,
    reason: PauseReason,            // AwaitingInput | AwaitingApproval | …
    payload: serde_json::Value,     // the handoff struct from §6.4
    paused_at: DateTime<Utc>,
}
```

Cloud needs a matching `on_run_paused` handler in
`apps/api/pi_dash/runner/consumers.py` (next to `on_run_completed` /
`on_run_failed` at line 236). The handler transitions the run to
`PAUSED_AWAITING_INPUT`, stores the payload on `done_payload`, and
triggers `drain_for_runner(runner)` so the runner picks up its next
queued work immediately.

**Claude does not use this path.** Claude's yield rides the existing
`pi-dash-done` fenced-block channel (§4.4); cloud's
`done_signal.ingest_into_run` is the entry point, with the new `"paused"`
status mapping to `PAUSED_AWAITING_INPUT`. No new `ClientMsg` variant is
needed for Claude.

### 7.3 Resume-failure reporting

When the agent CLI returns "no such session" or equivalent on a resume call,
the runner reports it via `RunFailed` with a new
`FailureReason::ResumeUnavailable`. Cloud's reaction is described in §6.3.

## 8. Recommended MVP Shape

### 8.1 Cloud (Django)

1. **Status enum + classifiers**: add `PAUSED_AWAITING_INPUT` to
   `AgentRunStatus`. Update **all five** classifiers consistently per the
   table in §4.3:
   - Out of: `BUSY_STATUSES`, `is_terminal`, `is_active`,
     `ACTIVE_RUN_STATUSES`.
   - In: `NON_TERMINAL_STATUSES`.
2. **Done-signal contract** (`apps/api/pi_dash/orchestration/done_signal.py`):
   add `"paused"` to `VALID_STATUSES`. In `ingest_into_run`, map
   `"paused"` → `PAUSED_AWAITING_INPUT` (non-terminal); leave `"blocked"`
   → `BLOCKED` unchanged. Leave `done_payload` populated so the handoff
   is preserved across resume.
3. **Schema**: add `pinned_runner_id` (FK to `Runner`, nullable) on
   `AgentRun`. Field-only migration.
4. **Comment trigger**: `post_save` on `IssueComment`:
   - Filter `comment.actor.is_bot` (excludes `pi_dash_agent` workpad bot).
   - Apply state gating per §5.2 (only continue when issue.state.group is
     STARTED).
   - Coalesce duplicate triggers: if a QUEUED follow-up already exists for
     the chain, append the comment to its prompt rather than creating a
     second run.
   - Create R_next with `pinned_runner_id = R_prev.runner_id` (only when
     `R_prev.thread_id` is not null and `R_prev.runner` is online-eligible).
5. **Matcher**: rewrite `drain_pod` to be runner-first; introduce
   `next_for_runner` with the personal-then-pod query from §5.4.
6. **Drain triggers**: add `drain_for_runner` calls in the Channels consumer
   on `run_completed`, `run_failed`, on the new `run_paused` handler, and
   on `on_hello` connect (after `_resume_run`). Also from
   `done_signal.ingest_into_run` when status becomes
   `PAUSED_AWAITING_INPUT` (Claude path).
7. **`on_run_paused` consumer handler** (`runner/consumers.py:236` neighborhood):
   transition the run, persist `done_payload`, post the handoff as an
   issue comment (§6.4), and trigger drain.
8. **Terminate-side comment sweep** (`runner/consumers.py` terminate handlers):
   in `on_run_completed`, `on_run_failed`, and `on_run_paused`, after
   updating run state, check for non-bot `IssueComment`s with
   `created_at > R.started_at` on the run's issue. If any exist, invoke
   the comment-trigger flow (§5.2) to create R_next. No new model field
   or pending-comment table — `IssueComment` + the timestamp range is
   the storage.
9. **Assign payload**: include `resume_thread_id = R.parent_run.thread_id`
   when set; `pinned_runner_id` does not need to be on the wire.
10. **Pin release on revoke**: on `Runner` REVOKED transition, null out
    pinned QUEUED runs. Trigger drain on the affected pod.
11. **Operator pin-release**: a UI/API affordance to manually clear
    `pinned_runner_id` on a stuck QUEUED run.

### 8.2 Runner (Rust)

1. **Supervisor**: read `resume_thread_id` from the incoming `Assign`
   message and plumb it into `RunPayload` instead of the hardcoded `None`
   at `runner/src/daemon/supervisor.rs:544`.
2. **Claude Code resume**: lift the "single-turn only" restriction in
   `runner/src/claude_code/mod.rs:10`; pass `--resume <session_id>` when
   `resume_thread_id` is set. Detect "no such session" failure and bubble
   it up as `RunFailed { reason: ResumeUnavailable }`.
3. **Codex resume**: already wired in `runner/src/codex/bridge.rs:61-72`.
   Verify that `RunPaused` paths leave the Codex thread state intact on
   disk and that subsequent `thread/resume` works.
4. **Codex yield tool**: expose `request_user_input` via the Codex
   JSON-RPC tool surface. On tool call, send `ClientMsg::RunPaused { ... }`
   and return runner to idle.
5. **Claude yield**: no runner-side change. Claude emits the existing
   `pi-dash-done` fenced block with the new `"paused"` status; the runner
   already forwards Claude's terminal output and cloud parses it.
6. **Resume-failure detection**: when the bridge's resume call fails
   because the session is missing, report `RunFailed { reason:
ResumeUnavailable }`.

### 8.3 Prerequisite for Claude continuation

Claude approvals are bypassed today (`runner/src/claude_code/mod.rs:6-10`,
`runner/src/claude_code/bridge.rs:122-128` fails fast on approval routing).
This is **not a blocker for the yield path** above — Claude yields via
done-signal, not via tool-call/approval — but it does mean Claude
continuation lacks the structured tool-yield UX Codex gets. Wiring the
permission-prompt MCP bridge is tracked separately in `claude_code/mod.rs`'s
MVP-limitations comment; if/when it lands, Claude can adopt the same
tool-call yield mechanism Codex uses, and the done-signal `"paused"` path
becomes a fallback rather than the primary mechanism.

### 8.3 Defer

- Mid-run inbound user messages (forwarding a comment into a `RUNNING` run).
  Workaround: the terminate-side timestamp sweep (§5.2) picks up comments
  with `created_at > R.started_at` once the run finishes, and dispatches
  R_next normally. No separate pending-comment storage.
- Transcript-replay fallback (§6.5).
- `IssueConversation` entity (one-thread-per-issue is sufficient for v1).
- TTL-based pin release (§5.7) — operator-driven is enough for v1.
- Cross-issue / cross-conversation memory.
- Smarter prompt for fresh-context-from-issue beyond "read the issue + the
  prior handoff comment."

## 9. Open Questions

| #   | Question                                                                                                                                                                                                                        |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Q1  | What's the right TTL on a `PAUSED_AWAITING_INPUT` run? After 7 days with no comment, do we GC it to terminal?                                                                                                                   |
| Q2  | If R_prev terminated SUCCEEDED but a comment still arrives weeks later, do we resume? Probably yes, but the agent CLI's session may have been cleaned up.                                                                       |
| Q3  | Workspace persistence on the runner: when agentA pauses issue001 and switches to issue002 (different workspace path), can issue001's workspace be left dirty? Or do we require commit-before-yield?                             |
| Q4  | If the issue is no longer in `In Progress` (user moved it back to Backlog), do we still dispatch on comment? Probably not — define the gating rule.                                                                             |
| Q5  | Billing: a follow-up run costs tokens; same `runner.owner` semantics as the primary run, or attribute to the comment author?                                                                                                    |
| Q6  | Do we expose conversation history (the run chain) as a first-class API for the UI's issue panel, or keep it derived (`issue.agent_runs.order_by(...)`)?                                                                         |
| Q7  | The handoff payload schema (§6.4) — minimal viable shape, or richer fields (file-level annotations, follow-up checklist)?                                                                                                       |
| Q8  | Pinned-runner failure UX: when run sits QUEUED because the pinned runner is offline, what does the user see? Silent wait, or a "stuck on agentA" indicator with a release button?                                               |
| Q9  | Terminate-side comment sweep (§5.2): exact filter semantics. Use `created_at > R.started_at` or `> R.created_at`? Exclude the comment that originally triggered R itself? Cap the lookback for very long-running paused chains? |

## 10. Summary of Decisions

The original framing put the central tradeoff as **runner affinity vs. queue
fairness**, and recommended soft pin with replay fallback. After working
through interleaving and where state lives, the decisions tighten:

1. **Strict pin, not soft pin.** Comment-triggered runs go back to the same
   runner. No grace window, no fallback to "anyone." Native resume is the
   correctness path; everything else is recovery from a real failure.
2. **Paused is not busy _or_ active.** Add `PAUSED_AWAITING_INPUT` to the
   status enum and place it correctly across all five classifiers
   (`BUSY_STATUSES`, `NON_TERMINAL_STATUSES`, `is_terminal`, `is_active`,
   `ACTIVE_RUN_STATUSES`) — not just the two named in earlier drafts.
   Excluding it from `is_active` is what lets the comment trigger create
   a follow-up; excluding it from `BUSY_STATUSES` is what lets the runner
   keep working other issues while paused.
3. **`"paused"` is a new agent done-signal status, distinct from `"blocked"`.**
   The existing `"blocked"` → terminal `BLOCKED` contract stays intact.
   Adding `"paused"` extends the done-signal grammar without disturbing
   anything that already reads BLOCKED.
4. **Yield mechanism diverges per agent.** Codex uses a `request_user_input`
   tool call and a new `RunPaused` `ClientMsg` (the approval path is wired,
   tool calls are first-class). Claude rides the existing `pi-dash-done`
   fenced block with the new `"paused"` status because Claude approvals
   are bypassed and there's no tool-call surface to piggyback. Both paths
   converge on the cloud-side `PAUSED_AWAITING_INPUT` state.
5. **One table, two filters.** No new queue, no new data structure — just
   `pinned_runner_id` on `AgentRun` and a smarter `ORDER BY`.
6. **Runner-first dispatch.** When a runner becomes idle, ask "what should
   you take?" rather than "is there work that fits you?". Eliminates
   head-of-line blocking on pinned runs and serves personal-queue work the
   moment the runner frees up.
7. **Cloud is authoritative; runner is stateless about past runs.** No new
   runner-side index. The agent CLI's own session store is the on-disk
   resume artifact.
8. **Handoff comments, not transcript replay.** When native resume fails,
   fresh-context-from-issue carries it — _if_ the yield protocol forces a
   structured handoff. This sidesteps the entire replay-quality problem.
9. **Comment trigger has explicit gates.** Bot filter on `actor.is_bot`
   (the `pi_dash_agent` workpad bot is the concrete blocker), state-group
   gating to STARTED, and coalesce on duplicate QUEUED followers. Without
   these, the trigger conflicts with the existing single-active-run
   guardrail and the workpad bot self-triggers a loop.

The cost of this design: roughly ~200 lines of cloud Python (status +
classifiers, column, done-signal extension, matcher rewrite, consumer
hooks, comment signal with gates), one wire-protocol field addition,
one new `ClientMsg` variant for Codex yield, and two runner-side changes
(Claude `--resume` plumbing, Codex yield tool). No new tables, no new
services, no new background workers.

## 11. Expected Behavior

This section captures the user-visible and system-level behavior the
implementation must produce. Each scenario maps to an integration-test
candidate in §11.5.

### 11.1 Happy paths

**A1. Issue completes in one run.** Unchanged from today. State → In
Progress, run dispatched, agent emits `pi-dash-done` with
`status: "completed"`, run terminates COMPLETED. No continuation.

**A2. Issue takes two runs (clean continuation).**

1. Issue → In Progress; run R1 dispatched to agentA.
2. Agent yields with a question. R1 → `PAUSED_AWAITING_INPUT`. The
   handoff payload is posted as an issue comment so the human can read
   the question.
3. AgentA returns to idle. Pod treats it as available (paused ≠ busy).
   User-visible: "Run paused — agent has a question."
4. User comments with their answer.
5. Cloud creates R2 pinned to agentA, with
   `resume_thread_id = R1.thread_id`. R2 lands in agentA's personal
   queue.
6. AgentA picks R2 up the next time it's idle (immediately if no other
   work; after current run if busy with another issue).
   User-visible: "Run resumed."
7. Agent continues the same Codex/Claude session, finishes, R2 →
   COMPLETED.

**A3. Three-way interleave.** This is the canonical scenario the design
is built around.

1. R1 paused on agentA after a yield.
2. AgentA picks issue002's R3 from pod queue → busy on R3.
3. User comments on issue001 → R2 created, pinned to agentA, sits
   QUEUED.
4. AgentA finishes R3 → `drain_for_runner` checks personal queue
   first → picks R2 → resumes.
5. From the user's POV: their comment didn't get an immediate response,
   but the moment agentA freed up it picked up where it left off — with
   full session memory, not a re-read of the issue.

### 11.2 Timing expectations

| Event                                   | Expected latency                                                  |
| --------------------------------------- | ----------------------------------------------------------------- |
| Yield → handoff comment visible         | Sub-second (posted in `on_run_paused` / done-signal ingest)       |
| Comment → dispatch (idle pinned runner) | Sub-second (`on_commit(drain_for_runner)`)                        |
| Comment → dispatch (busy pinned runner) | As long as the runner's current run takes; no fallback timeout    |
| Comment during RUNNING → dispatch       | At terminate of R_prev (no extra delay beyond terminate handling) |

### 11.3 Coalescing & ordering

- **Two comments before R2 dispatches.** Appended into a single R2
  prompt. User does not get two parallel runs.
- **Multiple paused issues on agentA.** When agentA frees up, FIFO by
  `R_next.created_at` (the follow-up's creation time, not the
  original's). Earliest comment-trigger wins.
- **Bot comments.** Filtered by `actor.is_bot`. The `pi_dash_agent`
  workpad updates never trigger continuation.

### 11.4 State-gating outcomes

| Issue state when comment arrives | Expected behavior                                              |
| -------------------------------- | -------------------------------------------------------------- |
| STARTED ("In Progress")          | Comment triggers continuation (the main path)                  |
| BACKLOG / UNSTARTED              | Ignored. State-transition path is what starts work             |
| COMPLETED / CANCELLED            | Ignored. User must move issue back to In Progress to re-engage |
| Custom workspace state group     | Ignored (conservative MVP); revisit per Q4                     |

### 11.5 Failure-mode behavior

**B1. Pinned runner offline.** R2 sits QUEUED with
`pinned_runner = agentA` indefinitely. UI surfaces a "stuck on agentA —
release pin?" indicator. Operator clicks "release pin" →
`pinned_runner_id = NULL` → drain places R2 on whichever runner is
free → resume fails on a different runner → `ResumeUnavailable` → R2
re-queued without `resume_thread_id` → fresh-context dispatch using the
issue + handoff comment.

**B2. AgentA revoked while R2 is pinned.** Automatic. The
`Runner.status = REVOKED` transition nulls `pinned_runner_id` on QUEUED
runs in the affected pod and triggers drain. R2 falls into the pod
general queue. Same fresh-context fallback as B1.

**B3. Resume fails (session evicted from disk).** AgentA online, R2
dispatched to it, `claude --resume <id>` (or Codex `thread/resume`)
returns "no such session." Runner reports
`RunFailed { reason: ResumeUnavailable }`. Cloud drops the pin and the
resume hint, re-queues R2. Picked up fresh by any runner — possibly
agentA itself — with the handoff comment as context.

**B4. AgentA's host was reinstalled.** Same as B3; session store was
wiped, detected the same way.

**B5. Yield with malformed handoff payload.** Existing
`done_signal.ingest_into_run` returns FAILED on parse error. For the
new `"paused"` status, validation must additionally reject when the
required handoff fields (e.g., `autonomy.question_for_human`) are
absent. Run terminates as FAILED, not PAUSED.

### 11.6 Invariants the implementation must preserve

1. **PAUSED status placement** across all five classifiers per §4.3:
   out of `BUSY_STATUSES`, `is_terminal`, `is_active`,
   `ACTIVE_RUN_STATUSES`; in `NON_TERMINAL_STATUSES`.
2. **Single-active-run guardrail.** `_active_run_for` continues to
   block creation of two `is_active` runs for the same issue. A paused
   run + one QUEUED follow-up is allowed (paused is not active); two
   QUEUED followers are not (coalesce step).
3. **Pin only when resume is meaningful.** Set `pinned_runner_id` only
   when `R_prev.thread_id IS NOT NULL` and `R_prev.runner` is
   online-eligible. Otherwise leave NULL so any runner can take R_next.
4. **Pod deletion respects paused runs.** Pod deletion path refuses
   while any non-terminal run (including PAUSED) exists in the pod.
   The drain process must explicitly resolve paused runs (cancel or
   re-route) before the pod can be removed.
5. **Bot self-trigger immunity.** No agent-authored comment can
   produce a new run. Filter must be on `actor.is_bot=True`, not on
   string match or username.

### 11.7 What does NOT change

- The pod queue model. Fresh issues still dispatch to whoever's free
  in the pod.
- Runner identity exposure. No new "this run is pinned to that runner"
  UI beyond the stuck-pin indicator from B1.
- Existing terminal statuses (`COMPLETED`, `FAILED`, `CANCELLED`,
  `BLOCKED`). `BLOCKED` keeps its terminal contract; `"paused"` is the
  new agent status that produces the new non-terminal
  `PAUSED_AWAITING_INPUT`.
- Runner-side persistence model. No new index, no new cache, no new
  per-issue state on the runner.

### 11.8 Integration test scenarios

These map directly to acceptance criteria for the implementation:

1. **Yield → comment → resume on idle runner.** A2 end-to-end with
   agentA being the only runner in the pod.
2. **Yield → other-issue dispatch → comment → resume after current
   run.** A3 end-to-end. Verify R2 sits QUEUED while agentA works on
   issue002 and dispatches the moment R3 terminates.
3. **Coalesce two comments into one follow-up.** Send two comments in
   rapid succession before R2 dispatches; assert only one new
   `AgentRun` row exists and its prompt contains both comments.
4. **Bot comment does not trigger continuation.** Workpad update from
   `pi_dash_agent`; assert no new `AgentRun` is created.
5. **State gating.** Comment on a BACKLOG / COMPLETED issue produces
   no new run.
6. **Comment during RUNNING.** Comment arrives mid-run; assert the
   terminate-side sweep picks it up and dispatches R_next when R_prev
   terminates.
7. **Stuck pin → operator release → fresh-context fallback.** Take
   pinned runner offline, simulate operator release, assert R2 routes
   to another runner without `resume_thread_id`.
8. **Resume failure → automatic fallback.** Mock the agent CLI's
   resume call to fail; assert cloud drops the pin and re-queues
   without resume hint.
9. **Status classifier coverage.** Unit test asserting
   `PAUSED_AWAITING_INPUT` membership in each of the five
   classifiers per §4.3.
10. **Pod deletion blocked by paused runs.** Attempt to delete a pod
    with a `PAUSED_AWAITING_INPUT` run; assert it's refused until the
    run is resolved.
11. **Multiple paused issues on one agent, FIFO by follow-up
    creation.** Pause issue001 then issue003 on agentA; comment on
    issue003 first, then issue001; assert R3-next dispatches before
    R1-next when agentA frees up.
12. **PAUSED + state transition back to In Progress.** With a paused
    run on the issue, move the issue to In Progress manually; assert
    the resulting run is pinned to the prior runner with
    `resume_thread_id` set (per Q-style edge case from §11.6 #2).
