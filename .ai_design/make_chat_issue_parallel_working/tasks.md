# Tasks: Parallel Chat + Issue Execution

> Companion to [`design.md`](./design.md). Phase 1 is the shippable MVP. The
> ordering matters: land the shared-state fixes (P1.A) **before** removing the
> guards (P1.C), or concurrent lanes will emit contradictory status and risk a
> stall-watchdog mis-fire (design §3.3). All `file:line` refs are from the
> design review — re-confirm at implementation time.

## Phase 1 — Concurrent chat + issue on pooled runners (MVP)

### 1.A Shared per-runner state fixes (land FIRST, behind the guards)

These are safe to land while the guards still serialize the lanes; they just make
the state model correct for when the guards come off.

- [ ] **B1 — derive `RunnerStatus`.** In `state.rs`, compute reported status from
      `run_active || chat_active` (+ `Reconnecting`/`AwaitingReauth` overrides)
      instead of direct writes. Add a `chat_active` signal to `StateHandle` fed by
      `CurrentChat.active_rx`. Remove the direct `set_status(Idle/Busy)` calls in
      `ChatWorker` (`supervisor.rs:1428/1459/1485/1808/1885/1931/1979`).
- [ ] **B1 test** — heartbeat never reports `status:"idle"` with non-null
      `in_flight_run`; status is `Busy` if _either_ lane is active.
- [ ] **B2 — observability ownership.** Make `ObservabilitySnapshot` explicitly
      assign-lane-owned; assert/guard that the chat lane writes no snapshot
      fields. (`state.rs:132`, `set_current_run` reset `:228–235`.)
- [ ] **B3 — single derived `approvals_pending`.** Compute the count once from the
      shared `ApprovalRouter` after any change; remove the per-lane
      `set_approvals_pending` stamps (`supervisor.rs:1904/1935`, `:3005/3049`).
- [ ] **B3 test** — concurrent chat + assign approvals report the _combined_
      count; the assign stall-watchdog (`pump_events`, `:2745`) does not mis-fire
      when only a chat approval is pending.
- [ ] **B4 — chat shutdown signal.** Add `state.shutdown_notified()` arm to the
      `ChatWorker` loop (`:1389–1479`); `RemoveRunner` (`:1159`) and daemon
      shutdown send `ChatCommand::Close`/`Shutdown` and emit a terminal
      `ChatClosed`; persistent worktree is left intact.

### 1.B Dedicated chat worktree provider (design §3.1, §3.6, §3.7)

- [ ] Add a `chat_worktree` provider in `runner/src/workspace/` that lazily
      creates one worktree **per runner** under `chat-worktrees/<runner_id>`
      (**outside** the pool's `wt-<int>` scan, `pool.rs:814`) via
      `git::worktree_add` (`git.rs:136`).
- [ ] `resolve_chat_workspace` (pooled runner, `supervisor.rs:2027–2046`) returns
      this dedicated path and **no longer** calls `pool.acquire(LeaseKind::Session)`
      — worktree lifetime is decoupled from `ChatWorker` lifetime, so a
      re-spawned worker for the same runner reuses the same path/state (§3.7).
- [ ] Keep the cloud-supplied `cwd` handling as a **sub-path within** the chat
      worktree (reuse the existing escape-rejection in `resolve_chat_workspace`);
      cwd binds once per session — no per-turn re-rooting needed (§3.6).
- [ ] **Bespoke** startup recovery (graft at the instance loop near pool spawn,
      `supervisor.rs:121`/`:199`, before `RunnerLoop` starts): `git worktree
  prune` + verify `chat-worktrees/<runner_id>` is healthy (`git -C … rev-parse`)
      → **reuse as-is if healthy (keep dirty state)**, `worktree remove --force` +
      recreate-lazily if broken. **No salvage/reset/clean** (that would wipe the
      persistent state — §3.7).
- [ ] Confirm legacy `pool.is_none()` runners are unaffected (chat stays in
      `working_dir`, serialized per §3.5).

### 1.C Remove the cross-lane guards (pooled runners only)

- [ ] `supervisor.rs:937`/`:1009` — stop rejecting `ChatUserMessage`/`ChatWarm`
      with `runner_busy` when `current_run.is_some()` **and `pool.is_some()`**.
- [ ] `supervisor.rs:809` — stop ignoring `Assign` when a chat is active (pooled).
- [ ] `supervisor.rs:820` — stop tearing down an idle chat for an `Assign`.
- [ ] **Keep** `:823` (one issue in flight at a time) and keep all guards for
      **legacy `pool.is_none()` runners** (design §3.5).
- [ ] Confirm the `tokio::select!` completion arms (`:701–712`) behave with both
      lanes live (the existing `biased` ordering is sound — verify with a test).

### 1.D Intra-chat turn serialization

- [ ] Confirm the `ChatCommand` mpsc queue already serializes chat turns (one
      write turn at a time in the chat worktree). Add a guard/test if not.

### 1.E Cloud-side (apps/api)

- [ ] `views/chat.py:299–300`, `:242–243` — drop the run-activity / `BUSY` half of
      the chat gate so chat is accepted while an issue runs (design §3.4).
- [ ] `services/matcher.py` (`~:121/:234/:287`) — remove
      `.exclude(pk__in=_runners_with_active_chat_ids())`; **keep**
      `agent_runs__status__in=BUSY_STATUSES`.
- [ ] Re-confirm `reap_stale_busy_runs` (`session_service.py:77–167`) is unaffected
      (chat is not an `AgentRun`; assign lane stays single-tenant).
- [ ] Audit other readers of `Runner.status == busy` for "idle-for-chat"
      assumptions (design §9.3).

### 1.F Web (apps/web)

- [ ] `runners/chat/[runnerId]/page.tsx:76–84` — `disabledReason()` stops treating
      `runner.status === "busy"` as a chat blocker.
- [ ] Surface "chatting + working" as a second signal (e.g. via
      `runner-agent-status-panel.tsx`, which already reads `RunnerLiveState`)
      rather than overloading the single status badge.

### 1.G Integration tests (runner)

- [ ] Chat turn accepted while an issue run is in flight (no `runner_busy`),
      pooled runner.
- [ ] Issue assignment accepted while a chat is active, pooled runner.
- [ ] Chat writes land in the dedicated chat worktree; issue writes land in a pool
      desk — assert disjoint paths.
- [ ] Multi-turn chat writes accumulate in the **same** chat worktree (the §2.3
      pollution scenario cannot recur).
- [ ] A runner that never chats never creates a chat worktree (lazy).
- [ ] Legacy `pool.is_none()` runner still serializes chat vs issue (guard kept).
- [ ] Reported status is `Busy` whenever either lane is active; never
      `idle + in_flight_run`.

## Phase 2 — Clean-session lifecycle (DONE, runner-side)

- [x] Fresh branch from default on session start, **per-runner namespaced**
      (`chat/<runner_id>/<chat_session_id>`), §3.1.1 — `chat_worktree::start_session`.
- [x] Mandatory commit + push to remote on session end; never local-only —
      `chat_worktree::end_session` / `git::commit_and_push_all`.
- [x] Session lifetime 5 min idle; revive resumes the pushed branch (the branch
      name is deterministic from the session id, so revive needs no cloud
      coordination).
- [ ] Auto-create a PR only when the operator explicitly asks. **Cloud-side, not
      runner.** The runner now pushes the chat branch; turning it into a PR (and
      detecting "the operator asked") mirrors the issue-run → GitHub-sync path and
      belongs in `apps/api` + the web UI. Out of scope for the runner change.

## Phase 3 — Hygiene & ergonomics (backlog — see status, NOT a coherent build phase)

- [~] Per-lane `ObservabilitySnapshot`. **Effectively already satisfied / YAGNI.**
  The snapshot is assign-lane-owned and the chat lane writes none (B2). A full
  per-lane split is only meaningful once chat-agent telemetry is _collected_,
  which isn't planned and would also be a wire-protocol change with no consumer.
- [ ] TTL cleanup for chat branches. **Cloud-side / policy.** Only sessions that
      _wrote_ push a branch, and those carry real user work — auto-deleting them
      from the runner is unsafe. The cloud knows PR/merge status and should own a
      long-TTL sweep. Building destructive remote-branch deletion in the runner
      would be the wrong call.
- [ ] Read-only concierge mode + cross-worktree reads. **Large, optional.** Design
      §3.1 concluded read-only enforcement is _not needed_ (chat owns its tree).
      This is an opt-in mode requiring per-agent sandbox/permission work
      (codex sandbox, claude permission mode, approval policy) — a feature in its
      own right, only if a product reason emerges.
- [ ] Build-cache sharing / `node_modules` symlink. **Speculative optimization
      (YAGNI).** Only worth it for large projects with heavy per-worktree build
      artifacts; build it when a real disk problem is observed, not before.
- [ ] Concurrent lanes for legacy (non-pooled) runners. **Needs new infra.**
      Non-pooled runners have no canonical clone / pool, so the dedicated-worktree
      approach doesn't apply; giving them worktree isolation is an architecture
      change the design deliberately deferred (§3.5).

## Open questions

- [x] **Codex `app-server` cwd binding** — RESOLVED (design §3.6): chat binds cwd
      once per session, which all agents support; no per-turn re-rooting needed.
- [x] **Persistence vs. crash-reap** — RESOLVED (design §3.7): per-runner keyed,
      decoupled from `ChatWorker` lifetime, bespoke prune/repair that preserves
      dirty state.
- [ ] **Disk eviction (Phase 2/3)** — policy if many runners on one machine each
      hold a persistent chat worktree (idle-evict? cap?).
- [ ] **Per-runner vs per-(runner × agent kind)** chat worktree if a runner can
      switch agent kind (default: per runner).
