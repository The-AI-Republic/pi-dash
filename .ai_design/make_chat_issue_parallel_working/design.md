# Parallel Chat + Issue Execution: Dedicated Chat Worktree

> Directory: `.ai_design/make_chat_issue_parallel_working/`
>
> This design removes the runner's chat/issue mutual exclusion so an operator
> can chat with a runner **while** it executes an issue. Chat becomes its own
> lane with a **dedicated worktree**, running in parallel with the issue/assign
> lane on physically separate working trees. It builds on
> [`runner_direct_chat`](../runner_direct_chat/design.md) (which shipped chat
> with the explicit MVP limitation "a busy runner should not accept chat") and
> [`worktree_pooling`](../worktree_pooling/design.md) (the desk-lease model).
>
> **Status:** reviewed against the codebase (runner + cloud + web). §3.3, §3.4,
> §3.5 and §8 carry the concrete blockers the review surfaced. All `file:line`
> references are from the review and should be re-confirmed at implementation
> time.

## 1. Problem Statement

A runner does exactly one thing at a time. While executing an issue
(`AgentRun`), any chat message is rejected with `runner_busy` / "runner has an
active task". The reverse also holds: an active chat blocks new assignments.

The model we want is a **digital employee**: he never refuses to talk to the
boss just because he is mid-task. Chatting (talking with the boss) and issue
execution (working an assigned task) are independent and should run in parallel:

- the operator opens chat with a live runner at **any** time, even mid-issue,
- chat behaves like `codex` / `claude` in a terminal: some turns are read-only
  questions, some turns edit code,
- issue execution continues undisturbed on its own working tree.

### 1.1 Goals

- Chat and issue execution run **concurrently** on one runner.
- Chat is its **own lane / own agent conversation** — **not** funneled into the
  issue/assign lane.
- Chat can **read and write** code (terminal-style) with no risk of polluting or
  starving the issue worktrees.

### 1.2 Non-Goals (this iteration)

- Machine-level chat (one chat per dev machine). Rejected — §6.1.
- Concurrent chat+issue on **legacy non-pooled runners** — §3.5 (kept serialized).
- Mandatory commit/push lifecycle, session TTL, branch cleanup — deferred, §7.
- Steering a _running issue_ by injecting messages into its live session — §6.2.
- Per-lane agent **observability** parity (chat-agent PID/tokens telemetry) — §3.3.

## 2. Current System

### 2.1 The Connection Actor and Its Guards

`runner/src/daemon/supervisor.rs` runs one `RunnerLoop` per runner; the daemon
supervises all runners in one process (`supervisor.rs:98`, `:195`). Each loop
holds at most one of each lane and dispatches in a single `tokio::select!`
(`supervisor.rs:701–712`, with `wait_done` / `wait_chat_done` completion arms):

```rust
struct RunnerLoop {
    current_run: Option<CurrentRun>,    // issue/assign lane (AssignWorker)
    current_chat: Option<CurrentChat>,  // chat lane (ChatWorker)
    ...
}
```

The lanes already have **separate workers and separate cancel paths**. Mutual
exclusion is policy, enforced at three guards:

- `supervisor.rs:937` — `ChatUserMessage` rejected with `ChatFailed { code:
"runner_busy" }` when `current_run.is_some()` (also `ChatWarm`, `:1009`).
- `supervisor.rs:809` — `Assign` ignored when chat is _active_ (`active_rx == true`).
- `supervisor.rs:820` — _idle_ chat torn down for an `Assign`; `:823` — `Assign`
  ignored when a run is already in flight (**intra-assign-lane guard — keep**).

Cancel paths are already lane-isolated: `Cancel` → run only (`:903`),
`ChatCancel`/`ChatClose` → chat only (`:1080`/`:1092`).

### 2.2 Workspace: the Worktree Pool

`runner/src/workspace/pool.rs` provides pools of git worktrees ("desks"). **One
pool per `[[workdir]]`** (not per runner) — `supervisor.rs:116–136`; default
`pool_size = 2` (`config/schema.rs:191`), warn above 16 (`:196`). Desks live at
`<data_dir>/worktrees/<workdir>/wt-<id>` with locks under `…/locks/`. A startup
scan, `adopt_existing_desks` (`pool.rs:814`), claims any `wt-<int>` directory —
**so a dedicated chat worktree must not use that name/namespace** (§3.1).

Both lanes lease from this pool today via `PoolHandle::acquire(LeaseRequest)`:

- `AssignWorker` acquires a `LeaseKind::Run` desk for the issue run.
- `ChatWorker` lazily acquires a `LeaseKind::Session` desk on its first
  warm/turn (`resolve_chat_workspace`, `supervisor.rs:2027–2046`) and **holds it
  for the whole session**, released on `ChatWorker` drop (`:1481`).

The git primitives are **free functions** in `workspace/git.rs`, reusable
outside the pool: `worktree_add` (`:136`), `worktree_remove` (`:161`),
`worktree_prune`, `detach_head`, `reset_clean`, `salvage_wip`,
`checkout_work_branch`. The pool's owner task runs `setup_command` **inline**,
so a hung setup blocks every acquire/release for that workdir for up to
`SETUP_COMMAND_TIMEOUT = 15 min` (`pool.rs:45`).

### 2.3 Why Sharing the Pool Is the Root of the Hazard

Chat leasing from the shared pool, plus any per-turn release, produces:

1. **Cross-worktree pollution.** A chat that writes in turn 1 (desk 001),
   releases, then writes in turn 2 when only desk 002 is free, strands work in
   001 and continues in 002 — two dirty trees, neither correct.
2. **Pool starvation.** A held `Session` desk reduces `free_worktrees()`
   (`pool.rs:187`), which the cloud matcher reads as capacity
   (`supervisor.rs:270–276`); with `pool_size = 2`, one chat desk halves issue
   capacity.

### 2.4 Cloud Single-Activity Assumptions (apps/api)

The cloud enforces one-activity-per-runner in four places:

- **Chat send gate** — `views/chat.py:299–300`: rejects `409 runner_busy` when
  `runner_has_active_task(runner)` (any `AgentRun` in `BUSY_STATUSES`,
  `services/chat.py:83`) **or** `runner.status == BUSY`. Same on warm
  (`:242–243`).
- **Matcher eligibility** — `services/matcher.py` excludes runners with an active
  chat from issue assignment via `_runners_with_active_chat_ids()` (`:83–103`),
  applied at the three drain/select sites (`~:121`, `~:234`, `~:287`).
- **Reaper** — `session_service.reap_stale_busy_runs()` (`:77–167`) assumes a
  runner reports exactly **one** `in_flight_run` UUID and fails any other BUSY
  run. Chat is **not** an `AgentRun`, so the assign lane staying single-tenant
  keeps this valid.
- **`Runner.status`** — single enum `online/offline/busy/revoked`
  (`models.py:174`), set from the daemon's poll heartbeat
  (`views/sessions.py:345`); read by the matcher (excludes non-`ONLINE`) and the
  web UI.

## 3. Target Design

> **Chat = its own lane, with a dedicated worktree, running in parallel with the
> issue/assign lane.** The assign lane stays single-tenant (one issue at a time).

### 3.1 Dedicated Chat Worktree (managed outside the issue pool)

Give each runner **one dedicated chat worktree**, created and owned **outside**
the `PoolHandle`:

- **Lazily created** on first chat use (runners that never chat cost nothing).
- **Persistent** across chat sessions — terminal-style; whatever branch/dirty
  state a session leaves, the next resumes in.
- **Separate path namespace** — e.g. `<data_dir>/chat-worktrees/<runner_id>` or
  a `chat-<runner_id>` desk **outside** the pool's `wt-<int>` scan, so
  `adopt_existing_desks` never reaps it and it never counts against
  `free_worktrees()`.

**Mechanism decision — do NOT use a pool `Lease`.** A pool lease **cleans +
detaches HEAD on release** (`pool.rs:497–551`), which would wipe the persistent
chat state, and holding a `Session` lease reduces advertised capacity (§2.3).
Instead, manage the chat worktree directly with the `workspace/git.rs` free
functions (`worktree_add` once, reused thereafter). **Cost accepted:** crash
recovery / cleanup that the pool provides for free (salvage, prune of orphaned
worktrees on restart) must be re-implemented for the chat worktree (a small
adopt/prune-on-startup for the `chat-worktrees/` namespace). This is the
deliberate trade for persistence + zero pool-capacity impact.

This single decision dissolves the §2.3 hazards:

| Hazard                         | Resolution                                           |
| ------------------------------ | ---------------------------------------------------- |
| Cross-worktree pollution       | Only ever one chat worktree → no split across desks. |
| Pool starvation                | Outside the pool → no effect on `free_worktrees()`.  |
| Uncommitted work at end        | Irrelevant — the worktree persists, terminal-style.  |
| Read-only enforcement          | Not needed — chat owns its tree, full write access.  |
| Lazy lock-on-write             | Not needed — the dedicated tree is always available. |
| `SETUP_COMMAND_TIMEOUT` freeze | Outside the pool owner task → no interaction.        |

### 3.1.1 Per-runner, not per-workdir — and why

Multiple runners can reference the same `[[workdir]]` and share its issue pool
(§2.2). The chat worktree is nonetheless **per-runner**, not shared per-workdir.
This is mandatory, not a disk/safety tradeoff:

1. **Cross-runner write safety.** Runners R1 and R2 on the same workdir are
   independently chat-able at the same time. A chat tree they both wrote to would
   reintroduce the §2.3 cross-worktree pollution — just _between runners_ instead
   of between turns. Two concurrent writing chats need two trees, exactly as two
   issue runs need two desks.
2. **Per-runner persistence.** The chat worktree is identity-bound: R1 resumes
   its own branch/dirty state. A shared/anonymous tree cannot hold two runners'
   separate continuity.

The only way a shared per-workdir chat tree could be safe is to **serialize chat
across all runners on the workdir** ("can't talk to R2 while R1 is chatting") —
strictly worse UX and a partial return of the "refuse the boss" problem.
Rejected.

This is the mirror of the issue pool, which _is_ per-workdir (§2.2) precisely
because issue desks are **fungible capacity**; chat is bound to a **runner
identity**, so it is not poolable. Disk: per-runner means up to N trees for N
runners on a workdir, but it is lazy (only chat-written runners create one) and
idle-evictable (§9) — steady-state is one tree per _actively-used_ chat, not per
runner. A consequence for Phase 2: chat branch names must be **per-runner
namespaced** (e.g. `chat/<runner_id>/…`) so two runners on the same repo do not
collide on push.

### 3.2 Parallel Lanes

- Remove the **cross-lane** guards (`:937`/`:1009` chat-reject, `:809`
  assign-vs-active-chat, `:820` idle-chat-teardown). **Keep** the
  **intra-assign-lane** guard (`:823`, one issue in flight at a time).
- `AssignWorker` uses an **issue pool desk**; `ChatWorker` uses the **dedicated
  chat worktree**. Disjoint physical trees ⇒ safe concurrent file I/O.
- Within the chat lane, turns serialize (one conversation; the existing
  `ChatCommand` mpsc queue already guarantees this — confirm in §1.4 of tasks).

### 3.3 Concurrency Correctness — Shared Per-Runner State (the real blockers)

`ChatWorker` and `AssignWorker` are handed clones of one `StateHandle`. Today
mutual exclusion hides the fact that several per-runner singletons are written by
**both** lanes. Removing the guards exposes these races; each must be resolved.

**B1 — Reported `RunnerStatus` is a single flag written by both lanes
(BLOCKER).** `RunnerStatus` (`cloud/protocol.rs:385`) is `Idle | Busy |
Reconnecting | AwaitingReauth`, on one `watch::Sender`. The chat lane sends
`Busy`/`Idle` directly (`supervisor.rs:1428/1459/1485/…`), while the assign lane
sets it indirectly via `set_current_run` (`state.rs:238–243`, which sends `Idle`
on `None`). Concurrently, a chat turn finishing can stamp `Idle` while an issue
is still running (heartbeat then reports `status:"idle"` + `in_flight_run:<uuid>`
— contradictory), and `set_current_run(None)` can stamp `Idle` while a chat turn
is active.
**Resolution:** make the reported status **derived**, not directly written.
Track two booleans — `run_active` (already `current_run.is_some()` /
`rx_in_flight`) and `chat_active` (already `CurrentChat.active_rx`) — and compute
`reported = if run_active || chat_active { Busy } else { Idle }` (preserving
`Reconnecting`/`AwaitingReauth` overrides). Chat and assign lanes stop writing
the global status flag directly; they flip their own lane signal and the
StateHandle recomputes. This is a `state.rs` change plus removing the direct
`set_status(Idle/Busy)` calls in `ChatWorker`.

**B2 — `ObservabilitySnapshot` is one per-runner struct (BLOCKER for telemetry).**
`agent_pid`, `tokens`, `model`, `turn_count`, `agent_alive` live in one
`Mutex<ObservabilitySnapshot>` (`state.rs:132`); `set_current_run` resets it on
run-id change (`:228–235`). Two concurrent agents would clobber it. **MVP
resolution (chosen):** the snapshot remains **owned by the assign lane only**;
the chat lane writes **no** observability fields (it already writes none except
status, fixed by B1). Per-lane chat telemetry is an explicit non-goal (§1.2). Add
a guard/comment so a future chat-observability change can't silently stomp the
run snapshot. (Full fix later: namespace the snapshot per lane.)

**B3 — `approvals_pending` is a non-atomic read-then-write shared by both lanes
(BLOCKER).** Both lanes do `approvals.list_pending().await.len()` then
`state.set_approvals_pending(n)` (`supervisor.rs:1904/1935` chat, `:3005/:3049`
assign, `:925`/`:1120` decide). Concurrent stamps clobber; worse, an undercount
can let the assign lane's **stall watchdog** (`pump_events`, `:2745`) time out a
run because it sees no pending approval. The `ApprovalRouter` itself is safe
(keyed by unique `approval_id`, `router.rs:57/100`); only the derived count
races. **Resolution:** make `approvals_pending` a **single derived value** from
the shared router — have the StateHandle (or a single helper) recompute
`router.list_pending().len()` after any change, rather than each lane stamping
its own snapshot. Remove the per-lane `set_approvals_pending` stamps.

**B4 — Chat lane has no shutdown signal (BLOCKER for clean teardown).** Only
`AssignWorker::pump_events` selects on `state.shutdown_notified()`
(`supervisor.rs:2737`); the `ChatWorker` loop (`:1389–1479`) does not. On daemon
shutdown or `ServerMsg::RemoveRunner` (which cancels the run lane only, `:1178`),
the chat lane is aborted with no graceful `ChatClosed`, and its worktree handle
is dropped abruptly. **Resolution:** add a `shutdown.notified()` arm to the chat
loop and make `RemoveRunner` send `ChatCommand::Close`/`Shutdown`; on shutdown,
emit a terminal `ChatClosed` and leave the persistent worktree intact (do not
delete it — only prune on the next startup adopt if orphaned).

### 3.4 Reported-Status Reconciliation (cloud `BUSY` must stop meaning "no chat")

The cloud overloads `BUSY`: it means both "don't assign another issue" **and**
"can't chat". These must be split:

- **Keep** `BUSY`/`BUSY_STATUSES` gating **issue assignment** — the assign lane
  is still single-tenant, so the matcher must keep excluding runners with an
  in-flight `AgentRun` (`agent_runs__status__in=BUSY_STATUSES`) and the daemon
  may keep reporting `status: busy` while an issue runs.
- **Stop** `BUSY` (and `runner_has_active_task`) from blocking **chat**. Remove
  the run-activity half of the chat gate (`views/chat.py:299–300`, `:242–243`).
- **Stop** an active chat from blocking **issue assignment**: drop
  `.exclude(pk__in=_runners_with_active_chat_ids())` from the three matcher sites
  (`matcher.py ~:121/:234/:287`). The `drain_tasks_after_chat_release` coupling
  (`services/chat.py:90–115`) becomes unnecessary for _eligibility_ but is
  harmless.

Net rule after the change: **"Busy" = the assign lane is occupied → no second
issue. Chat is always acceptable** (subject only to liveness/`ONLINE`).

### 3.5 Legacy (non-pooled) runners — kept serialized

A runner with `pool: None` runs both lanes in the single `workspace.working_dir`
(`supervisor.rs:222–225`, chat at `:2054–2079`, assign at `:2489–2521`). There is
**no second tree to isolate**, so concurrent chat-write + issue-write would
collide. **Decision:** for non-pooled runners, **retain the current mutual
exclusion** (keep the guards' effect for `pool.is_none()`); concurrent lanes are
a **pooled-runner-only** feature in this iteration. The guard removal in §3.2
must therefore be conditional on `pool.is_some()`.

## 4. Lifecycle (MVP)

1. **First chat message** → lazily create the dedicated chat worktree (from the
   default branch) under `chat-worktrees/<runner_id>`; start `ChatWorker` bound
   to it. (Pooled runners only; legacy runners keep today's behavior.)
2. **Read turn** ("which branch is the repo on?") → agent answers; may
   dirty-read sibling issue desks (`git --no-optional-locks`).
3. **Write turn** ("switch to main, new branch, button red") → agent edits the
   **same** chat worktree; state accumulates across turns, terminal-style.
4. **Session goes quiet / closes** → worktree **kept** with its state; emit
   `ChatClosed`. No commit/push (deferred — §7).
5. **Issue assignment arrives mid-chat** (pooled runner) → accepted;
   `AssignWorker` runs on a pool desk in parallel. No `runner_busy`. Reported
   status is `Busy` (run_active) but chat remains accepted (§3.4).

## 5. Disk Cost Analysis

- Git worktrees **share** `.git/objects`; a chat worktree costs ≈ **one
  working-tree checkout** (tracked files at HEAD), not the repo/history. Heuristic:
  `du -sh <repo>` **minus** `.git`.
- **Lazy** (zero until first chat) and bounded to **one per runner** (reused
  across sessions, re-checked-out, never per-session).
- **Real cost driver:** per-worktree build artifacts (`node_modules/`,
  `target/`, `.venv/`). Mitigate with a shared build cache / symlinked deps for
  large projects (Phase 3).

**Verdict:** worth it — structural isolation makes the pollution/starvation bug
class _impossible_ rather than policy-managed, for one shared-object working tree.

## 6. Alternatives Considered

### 6.1 Machine-level chat (rejected)

One chat per dev machine (the `dev_machine_id` identity exists —
`runner_ops.rs:223`, `config/schema.rs:72`) overseeing all workdirs. Good for
reads, but **fails on write escalation**: it loses project context and must ask
"which project?" per task. Runner-scoped chat keeps project identity.

### 6.2 Steer the running issue (deferred)

Injecting the operator's message into the _live issue session_. Hard and
agent-specific. A runner-scoped chat can already dirty-read the issue worktree to
answer "how's it going?", recovering much value without live injection.

### 6.3 Chat as a pool `Lease` / dedicated 1-slot pool (rejected for MVP)

Reuses pool machinery (salvage, crash-reap) but the lease **cleans on release**,
which conflicts with cross-session persistence, and (for the shared pool) eats
capacity. §3.1 manages the worktree outside the pool instead.

### 6.4 Chat borrows from the issue pool (rejected)

Source of the §2.3 pollution/starvation hazards.

## 7. Deferred / Future Improvements

- **Clean-session lifecycle (Phase 2):** fresh branch from default on start;
  **mandatory commit + push** on end (nothing local-only); auto-PR only on
  request. Makes timed-out sessions reconstructable from the remote branch.
- **Session lifetime + revive (Phase 2):** ~5 min idle end; reviving restarts the
  countdown (codex history-resume UX).
- **Per-lane observability (Phase 3):** namespace `ObservabilitySnapshot` so chat
  agent telemetry doesn't collide with the run agent (B2 full fix).
- **Chat-branch cleanup TTL (Phase 3).**
- **Read-only concierge mode + first-class cross-worktree reads (Phase 3).**
- **Concurrent lanes for legacy runners (Phase 3)** — would need worktree infra
  on the non-pooled path, or a read-only-only chat there.

## 8. Affected Code

**Runner (OSS `pi-dash/`):**

- `runner/src/daemon/supervisor.rs`
  - Remove cross-lane guards `:809`, `:820`, `:937`/`:1009` **when `pool.is_some()`**;
    keep `:823` (intra-assign single-tenant) and the legacy serialized path (§3.5).
  - Bind `ChatWorker` to the dedicated chat worktree instead of acquiring a
    `LeaseKind::Session` desk (`resolve_chat_workspace`, `:2027–2046`).
  - Stop `ChatWorker` writing global status (`:1428/1459/1485/1808/1885/1931/1979`);
    flip a `chat_active` signal instead (B1).
  - Add `shutdown.notified()` arm to the chat loop (`:1389–1479`); `RemoveRunner`
    (`:1159`) sends `ChatCommand::Close` (B4).
- `runner/src/daemon/state.rs`
  - Derive reported `RunnerStatus` from `run_active || chat_active` (B1).
  - Single derived `approvals_pending` from the shared router (B3).
  - Guard `ObservabilitySnapshot` as assign-lane-owned (B2).
- `runner/src/workspace/` — a `chat_worktree` provider (lazy `git::worktree_add`
  once under `chat-worktrees/<runner_id>`, reuse, startup adopt/prune for orphans);
  **not** via `PoolHandle`.
- `runner/src/approval/router.rs` — no change to keying; expose a single
  authoritative pending-count read for B3.

**Cloud (`apps/api/`):**

- `views/chat.py:299–300`, `:242–243` — drop the run-activity / `BUSY` half of the
  chat gate (§3.4).
- `services/matcher.py` (`~:121/:234/:287`) — drop
  `.exclude(pk__in=_runners_with_active_chat_ids())`; **keep** the
  `agent_runs__status__in=BUSY_STATUSES` exclusion.
- `services/session_service.py` — no change (assign lane stays single-tenant;
  re-confirm the reaper only sees `AgentRun`s).

**Web (`apps/web/`):**

- `runners/chat/[runnerId]/page.tsx:76–84` — `disabledReason()` must stop
  treating `runner.status === "busy"` as a chat blocker.
- `core/components/runners/runner-status.ts` + list/detail badges — optionally
  surface "chatting + working" as two signals (the `runner-agent-status-panel`
  already reads `RunnerLiveState`, a good home for the chat signal).

See [`tasks.md`](./tasks.md) for the staged work breakdown.

## 9. Design-Review Verdict & Open Risks

**Verdict:** the dedicated-worktree direction is sound and the runner already has
separate workers + lane-isolated cancel, so the structure is most of the way
there. **However, it is NOT a guard-removal-only change.** The four B-blockers in
§3.3 (status derivation, observability ownership, approvals count, chat shutdown)
and the cloud `BUSY` reconciliation in §3.4 are mandatory; shipping guard removal
without them yields contradictory runner status, a stall-watchdog mis-fire risk,
and ungraceful chat teardown.

**Open risks to resolve during implementation:**

1. **Persistence vs. crash-reap.** Managing the chat worktree outside the pool
   means re-implementing orphan cleanup. Verify the startup adopt/prune handles a
   half-created or stale `chat-worktrees/<runner_id>` after a crash.
2. **Codex `app-server` cwd binding.** A codex _conversation_ may bind cwd at
   creation; confirm the chat conversation can be (re)rooted in the dedicated
   worktree without losing context. Affects only the codex agent.
3. **Status semantics across the fleet.** Confirm no other cloud consumer treats
   `Runner.status == busy` as "idle for chat" beyond the sites in §3.4 (search for
   `RunnerStatus.BUSY` / `status="busy"` readers).
4. **Heartbeat consistency.** After B1, verify the poll body never emits
   `status:"idle"` with a non-null `in_flight_run` under concurrent lanes.
5. **Matcher capacity.** Since the chat worktree is outside the pool,
   `free_worktrees()` is unaffected — confirm no assumption that "a chatting
   runner has reduced capacity".
