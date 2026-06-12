# Parallel Chat + Issue Execution: Dedicated Chat Worktree

> Directory: `.ai_design/make_chat_issue_parallel_working/`
>
> This design removes the runner's chat/issue mutual exclusion so an operator
> can chat with a runner **while** it executes an issue. Chat becomes its own
> lane with a **dedicated, persistent worktree**, running in parallel with the
> issue/assign lane on physically separate working trees. It builds directly on
> [`runner_direct_chat`](../runner_direct_chat/design.md) (which shipped chat
> with the explicit MVP limitation "a busy runner should not accept chat") and
> [`worktree_pooling`](../worktree_pooling/design.md) (the desk-lease model).

## 1. Problem Statement

A runner today does exactly one thing at a time. When it is executing an issue
(`AgentRun`), any chat message is rejected with `runner_busy` / "runner has an
active task". The reverse also holds: an active chat blocks new assignments.

The mental model we want is a **digital employee**: an employee never refuses to
talk to the boss just because he is working on a task. Chatting (talking with
the boss) and issue execution (working on an assigned task) are independent
activities and should run in parallel:

- the operator opens chat with a live runner at **any** time, even mid-issue,
- chat behaves like using `codex` / `claude` in a terminal: some turns are
  read-only questions, some turns edit code,
- issue execution continues undisturbed on its own working tree.

### 1.1 Goals

- Chat and issue execution run **concurrently** on one runner.
- Chat is its **own lane / own agent conversation** — it is **not** funneled
  into the issue/assign lane.
- Chat can both **read** and **write** code (terminal-style), with no risk of
  polluting or starving the issue worktrees.

### 1.2 Non-Goals (this iteration)

- Machine-level chat (one chat per dev machine). Rejected — see §6.1.
- Mandatory commit/push lifecycle, session TTL, branch cleanup. Deferred — §7.
- Steering a _running issue_ by injecting messages into its live session — §6.2.

## 2. Current System

### 2.1 The Connection Actor and Its Guards

`runner/src/daemon/supervisor.rs` runs one `RunnerLoop` per runner (the daemon
supervises all runners on the machine in one process — `supervisor.rs:98`,
`:195`). Each loop holds at most one of each lane:

```rust
struct RunnerLoop {
    current_run: Option<CurrentRun>,    // issue/assign lane
    current_chat: Option<CurrentChat>,  // chat lane
    ...
}
```

The lanes already have **separate workers** (`AssignWorker`, `ChatWorker`) and a
single `tokio::select!` dispatch loop. The mutual exclusion is purely policy,
enforced at three guard sites:

- `supervisor.rs:937` — `ChatUserMessage` rejected with `ChatFailed { code:
"runner_busy", detail: "runner has an active task" }` when `current_run.is_some()`.
- `supervisor.rs:809` — `Assign` ignored when a chat is _active_
  (`chat.active_rx == true`).
- `supervisor.rs:820` — an _idle_ chat runtime is torn down to make room for an
  `Assign`; `supervisor.rs:823` — `Assign` ignored when a run is already in flight.

### 2.2 Workspace: the Worktree Pool

`runner/src/workspace/pool.rs` provides per-`[[workdir]]` pools of git
worktrees ("desks") leased via `PoolHandle::acquire(LeaseRequest)`. Both lanes
lease from the **same** pool today:

- `AssignWorker` acquires a desk for the issue run.
- `ChatWorker` acquires a `LeaseKind::Session` desk for its whole lifetime
  (`supervisor.rs:1365`, `:2040`).

Because git worktrees **share the repo object store** (`.git/objects`), a desk
costs ~one working-tree checkout, not a full clone (see §5).

### 2.3 Why Sharing the Pool Is the Root of the Hazard

Chat leasing from the shared pool, combined with any per-turn release, produces
two failure modes:

1. **Cross-worktree pollution.** A chat that writes in turn 1 (desk 001), then
   releases, then writes in turn 2 when only desk 002 is free, strands
   uncommitted work in 001 and continues in 002 — two dirty trees, neither
   correct. (This is why a chat desk must be _sticky_, never per-turn.)
2. **Pool starvation.** A chat holding a `Session` desk competes with issue runs
   for the same finite pool.

## 3. Target Design

> **Chat = its own lane, with a dedicated, persistent worktree, running in
> parallel with the issue/assign lane.**

### 3.1 Dedicated Chat Worktree

Give each runner **one dedicated chat worktree**, outside the issue pool:

- **Lazily created** on the first chat message for that runner (runners that
  never chat pay zero disk).
- **Persistent** — kept across chat sessions. It behaves like a long-lived
  terminal checkout: whatever branch/dirty state the last session left, the next
  session resumes in.
- **Separate namespace** from the issue pool desks, so chat writes physically
  **cannot** collide with, pollute, or starve issue runs.

This single decision dissolves every hazard from §2.3:

| Hazard                   | Resolution                                           |
| ------------------------ | ---------------------------------------------------- |
| Cross-worktree pollution | Only ever one chat worktree → no split across desks. |
| Pool starvation          | Dedicated tree, never borrowed from the issue pool.  |
| Uncommitted work at end  | Irrelevant — the worktree persists, terminal-style.  |
| Read-only enforcement    | Not needed — chat owns its tree, full write access.  |
| Lazy lock-on-write       | Not needed — the dedicated tree is always available. |

### 3.2 Parallel Lanes

- Drop the three guards (§2.1). `current_run` and `current_chat` coexist.
- `AssignWorker` uses an **issue pool desk**; `ChatWorker` uses the **dedicated
  chat worktree**. Different physical trees ⇒ safe concurrency.
- Within the chat lane, **turns serialize** (one turn at a time in the chat
  worktree) — naturally true for a single conversation; the only rule is "no two
  concurrent write turns in the chat worktree".

### 3.3 Status Model

`RunnerStatus::Busy` is currently a single flag. Split visibility into two lanes
so the cloud/UI can show "chatting **and** working" rather than one Busy state.
The assign lane keeps its existing BUSY semantics for run accounting; the chat
lane reports its own activity.

## 4. Lifecycle (MVP)

1. **First chat message** → lazily create the dedicated chat worktree (checked
   out from the default branch), start `ChatWorker` bound to it.
2. **Read turn** ("which branch is the repo on?") → agent answers; may dirty-read
   sibling issue desks (`git --no-optional-locks`). Nothing special.
3. **Write turn** ("switch to main, new branch, make the button red") → agent
   edits the **same** dedicated chat worktree. State accumulates coherently
   across turns (turn N+1 sees turn N's edits), exactly like a terminal.
4. **Session goes quiet / closes** → the chat worktree is **kept** with its
   current state. No commit/push ceremony in the MVP (deferred — §7).
5. **Issue assignment arrives mid-chat** → accepted; `AssignWorker` runs on a
   pool desk in parallel. No `runner_busy`.

## 5. Disk Cost Analysis

The dedicated worktree adds disk, so quantify it honestly:

- Git worktrees **share** `.git/objects`. Marginal cost of one chat worktree ≈
  **one working-tree checkout** (tracked files at HEAD), **not** the repo/history
  size. Heuristic: `du -sh <repo>` **minus** `.git`.
- It is **lazy** (zero cost until first chat) and the runner already pays
  per-worktree cost for issue desks — the chat tree is **+1 working tree**, an
  increment, not a new category.
- **Real cost driver:** per-worktree build artifacts (`node_modules/`, `target/`,
  `.venv/`), which each worktree regenerates and can dwarf the source on large
  projects.

**Mitigations / recommendation:**

- Use **one reusable chat worktree per runner** (re-`checkout` per session)
  rather than one-per-session → caps disk at +1 working tree regardless of
  session count.
- For large projects, share a build cache (or symlink `node_modules`) into the
  chat worktree to avoid duplicating artifacts.

**Verdict:** worth it. Structural isolation makes the pollution/starvation bug
class _impossible_ rather than policy-managed, for the price of one shared-object
working tree.

## 6. Alternatives Considered

### 6.1 Machine-level chat (rejected)

One chat per dev machine (the `dev_machine_id` identity already exists —
`runner/src/cli/runner_ops.rs:223`, `config/schema.rs:72`) overseeing all
workdirs. Attractive for reads (one codex app-server multiplexes conversations
across many cwds), but **fails on write escalation**: a machine-level chat loses
project context and must ask "which project?" on every task request. Keeping
chat **runner-scoped** preserves project identity, so chat→write needs no prompt.

### 6.2 Steer the running issue (deferred)

Injecting the operator's message into the _live issue session_ (true
"interrupt the worker at his desk"). Much harder and agent-specific (codex
app-server can queue a user turn; one-shot CLIs cannot accept input mid-turn
without cancel/resume). Note: a runner-scoped chat can already **dirty-read the
issue worktree** to answer "how's the task going?", which recovers much of the
value without live injection.

### 6.3 Chat borrows from the issue pool (rejected)

Source of the §2.3 pollution/starvation hazards. The dedicated worktree replaces
it and makes the isolation structural.

## 7. Deferred / Future Improvements

- **Clean-session lifecycle (Phase 2):** on session start, check out a fresh
  branch from default; on session end, **mandatory commit + push** so nothing
  sits local-only; auto-PR only when the operator asks.
- **Session lifetime + revive (Phase 2):** end a chat session after ~5 min idle;
  the user reviving by typing restarts the countdown (codex history-resume UX).
  Made trivially safe once Phase 2's commit+push guarantees the session is
  reconstructable from the remote branch + agent session id.
- **Chat-branch cleanup TTL:** garbage-collect chat-authored branches that never
  became PRs.
- **Read-only mode + cross-worktree reads:** an explicit read-only concierge
  posture and first-class "report each desk's branch" reads.

## 8. Affected Code

**Runner (OSS `pi-dash/`):**

- `runner/src/daemon/supervisor.rs` — drop the three guards (`:809`, `:820/:823`,
  `:937`); let `current_run` + `current_chat` coexist; bind `ChatWorker` to the
  dedicated chat worktree instead of a pool `Session` desk; per-lane status.
- `runner/src/workspace/` — a dedicated-chat-worktree provider (lazy create,
  persistent, separate from `pool.rs`'s issue desks); reuse the worktree
  add/remove plumbing.
- `runner/src/approval/` — chat lane keeps full write on its own tree (no
  read-only enforcement needed in MVP).

**Cloud (`apps/api/`, `apps/web/`):**

- Chat session acceptance must stop depending on runner-idle; surface
  "chatting + working" in the runner status UI.
- Existing chat protocol (`ChatUserMessage` / `ChatEvent`) is reused unchanged.

See [`tasks.md`](./tasks.md) for the staged work breakdown.
