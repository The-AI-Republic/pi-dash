# Tasks: Parallel Chat + Issue Execution

> Companion to [`design.md`](./design.md). Phase 1 is the shippable MVP:
> dedicated chat worktree + parallel lanes. Phases 2+ are the deferred
> improvements from design §7.

## Phase 1 — Dedicated chat worktree + parallel lanes (MVP)

### 1.1 Dedicated chat worktree provider

- [ ] Add a chat-worktree provider in `runner/src/workspace/` that, per runner,
      **lazily creates** one dedicated worktree (checked out from the default
      branch) on first chat use and **persists** it across sessions.
- [ ] Keep it in a **separate path namespace** from the issue pool desks so it
      can never be handed out as an `AssignWorker` desk.
- [ ] Reuse the existing worktree add/remove plumbing from `pool.rs`; do **not**
      route chat through `PoolHandle::acquire`.
- [ ] One reusable worktree **per runner** (not per session) — re-`checkout` the
      target branch on session start. (Bounds disk to +1 working tree.)

### 1.2 Bind ChatWorker to the dedicated worktree

- [ ] Change `ChatWorker` (`supervisor.rs`) to use the dedicated chat worktree
      instead of leasing a `LeaseKind::Session` desk from the pool.
- [ ] Remove the chat `Session` desk lease (`supervisor.rs:~2040`) from the chat
      path.

### 1.3 Remove the mutual-exclusion guards

- [ ] `supervisor.rs:937` — stop rejecting `ChatUserMessage` with `runner_busy`
      when `current_run.is_some()`.
- [ ] `supervisor.rs:809` — stop ignoring `Assign` when a chat is active.
- [ ] `supervisor.rs:820/:823` — stop tearing down chat / ignoring `Assign` on an
      in-flight run; allow `current_run` + `current_chat` to coexist.
- [ ] Keep the "one run in flight at a time" guard for the **assign** lane only.

### 1.4 Intra-chat turn serialization

- [ ] Ensure the chat lane runs **one turn at a time** in its worktree (no two
      concurrent write turns). Confirm the single-conversation command queue
      already guarantees this; add a guard/test if not.

### 1.5 Per-lane status

- [ ] Split single `RunnerStatus::Busy` visibility into assign-lane vs chat-lane
      activity; keep assign BUSY semantics for cloud run accounting.

### 1.6 Cloud-side

- [ ] Chat session acceptance must not depend on the runner being idle.
- [ ] Surface "chatting + working" in the runner status UI (`apps/web/`).
- [ ] Confirm `reap_stale_busy_runs` / run accounting tolerate a runner that is
      simultaneously chatting and running an issue.

### 1.7 Tests

- [ ] Runner: chat turn accepted while an issue run is in flight (no
      `runner_busy`).
- [ ] Runner: issue assignment accepted while a chat is active.
- [ ] Runner: chat write turns land in the dedicated worktree; issue writes land
      in a pool desk; assert disjoint paths.
- [ ] Runner: multi-turn chat writes accumulate in the **same** chat worktree
      (the §2.3 pollution scenario cannot recur).
- [ ] Runner: a runner that never chats never creates a chat worktree (lazy).

## Phase 2 — Clean-session lifecycle (deferred, design §7)

- [ ] On session start: check out a fresh branch from default (unless the user
      specifies otherwise).
- [ ] On session end: **mandatory commit + push** to remote; never leave work
      local-only.
- [ ] Auto-create a PR only when the operator explicitly asks in the query.
- [ ] Session lifetime: end after ~5 min idle; reviving by typing restarts the
      countdown (resume from remote branch + agent session id).

## Phase 3 — Hygiene & ergonomics (deferred)

- [ ] TTL cleanup for chat-authored branches that never became PRs.
- [ ] Read-only concierge mode + first-class cross-worktree reads ("report each
      desk's branch").
- [ ] Build-cache sharing (or `node_modules` symlink) into the chat worktree for
      large projects.

## Open questions

- [ ] Per-runner vs per-(runner × agent kind) chat worktree when a runner can
      switch agent kind? (Default: per runner.)
- [ ] Disk ceiling / eviction policy if many runners on one machine each create a
      persistent chat worktree.
- [ ] Codex `app-server`: does a chat conversation need a fixed cwd at creation
      (affecting how the dedicated worktree binds)? Verify against the codex API.
