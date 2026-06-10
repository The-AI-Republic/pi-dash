# Worktree Pooling — Design

A `pidash` daemon manages a **pool of git worktrees per work dir**, decoupling
runners (agent configurations) from working directories (execution resources).
N runners can share one work dir; concurrency is bounded by the pool size, not
the runner count. When every worktree is in use, assigned runs **wait in a
local queue** owned by the daemon instead of failing.

This supersedes the one-runner-one-working-dir binding enforced today by
config validation (`runner/src/config/schema.rs:452-476`, the
`DuplicateWorkingDir` / `NestedWorkingDir` errors), and builds on the
multi-runner daemon (`../n_runners_in_same_machine/design.md`): runner
instances, the shared cloud transport (wire v4 — per-runner HTTPS long-poll,
`runner/src/cloud/protocol.rs:6`), and single-tenancy per instance are all
unchanged.

The mental model (used throughout this doc):

> Pi Dash cloud is the **boss**, an issue is a **job task**, a runner is an
> **employee**, a work dir is an **office**, and a worktree is a **desk**. The
> boss assigns tasks to employees and keeps the books; the office manages its
> own desks. An employee with a task but no free desk waits — visibly, on the
> boss's books — until a desk frees up.

---

## 1. Goals

- **N runners per work dir.** A codex runner and a claude_code runner can
  serve the same repo checkout; issues route to a specific agent (see the
  companion agent-aware-routing work) without duplicating clones.
- **One canonical clone per repo per machine.** History is stored once;
  worktrees share its object database. Repo size stops being a reason to avoid
  multiple runners.
- **Worktree pool with leases.** A run leases a clean worktree at start,
  releases it at the end (success, failure, or cancel); the worktree is
  cleaned and returned to the pool.
- **Wait, don't fail.** When the pool is exhausted, runs queue locally in the
  daemon (FIFO per work dir) until a worktree frees up.
- **Cloud stays the source of record.** Queued-locally is a first-class,
  cloud-visible run state; cancel works while queued; the local queue is
  rebuildable from cloud state after a daemon restart.
- **Warm pools.** Cleaning preserves gitignored files (dependency installs,
  build caches) by default, so pooled worktrees stay cheap to reuse even in
  very large repos.

## 2. Non-goals

- **Concurrent runs within one runner instance.** Instances stay
  single-tenant (≤ 1 in-flight run). Concurrency still comes from multiple
  instances; the pool bounds how many of them can _execute_ at once.
- **Cloud-side modelling of desks.** The cloud never sees worktree identities,
  branch locks, or pool internals. It sees runner readiness, a per-run
  waiting state, and an aggregate capacity hint — nothing more.
- **Cross-machine work-dir identity.** A work dir is a per-machine resource.
  Two machines cloning the same repo are two unrelated offices.
- **Worktree pooling for non-git work dirs.** The pool requires a git repo;
  the existing single-dir behavior remains for anything else.
- **Idle-worktree eviction (disk reclaim by TTL).** Deferred; see §13.

## 3. Vocabulary

| Term                | Meaning                                                                                                                                                  |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Work dir**        | A named, daemon-managed entity (`[[workdir]]` in config): one repo on this machine. Owns the canonical clone, the pool, and the local queue. The office. |
| **Canonical clone** | The git repo at `workdir.path`. Holds the shared object database. Agents never execute in it.                                                            |
| **Worktree**        | A `git worktree` checkout off the canonical clone, materialized under the pool dir. The desk.                                                            |
| **Pool**            | The set of worktrees for one work dir, capped at `pool_size`. Worktrees are created lazily up to the cap.                                                |
| **Lease**           | Exclusive ownership of one worktree by one run or chat session, held from acquisition until cleanup completes. Crash-safe (on-disk lock file).           |
| **Local queue**     | Per-work-dir FIFO of runs that have been assigned to a runner but cannot acquire a lease yet.                                                            |
| **Branch lock**     | The git-enforced invariant that one branch is checked out in at most one worktree (including the canonical clone) at a time.                             |

## 4. Pool mechanics

### 4.1 Layout

```
data_dir/
  worktrees/
    <workdir-name>/
      wt-1/                  # a git worktree of the canonical clone
      wt-2/
      locks/
        wt-1.lock.json       # { lease_kind, run_id|session_id, runner_id, acquired_at }
        wt-2.lock.json
```

Worktrees live under `data_dir`, **not** inside the work dir path — nesting a
worktree inside the repo it belongs to recreates the path-collision hazard the
old validation existed to prevent. `workdir.worktrees_dir` overrides the
location (e.g. to keep worktrees on the same filesystem as a huge repo).

The canonical clone at `workdir.path` is the user's existing checkout —
typically the directory they already pointed a runner at today. After this
change, agents never run in it again; it serves the object database and the
user can keep using it by hand.

### 4.2 Initialization and lazy growth

The pool is initialized when the first runner binds to the work dir (daemon
startup or `pidash runner add`). Initialization verifies the canonical clone
exists (clone it via the existing `workspace::resolve` path if the dir is
empty), runs `git worktree prune`, and reaps stale locks (§8). It does **not**
materialize worktrees upfront.

Worktrees are created on demand: when a lease is requested, no clean worktree
is free, and `live worktree count < pool_size`, the daemon runs
`git worktree add <pool>/wt-<n> --detach` and then runs the work dir's
`setup_command` (if configured) once in the new worktree — this is where
`.env` provisioning and the first dependency install happen. Default
`pool_size` is **2**; editable per work dir in the TUI and CLI (§10).

### 4.3 Lease acquisition

A run (or chat session) can acquire a lease when **both** hold:

1. A clean worktree is free (or can be lazily created under the cap).
2. Its target branch is not currently checked out in any other worktree **or
   in the canonical clone** (the branch lock — git enforces this at checkout;
   the daemon checks first so the run queues instead of erroring).

Acquisition writes the lock file, checks out the run's work branch in the
leased worktree (`workspace::git::checkout_work_branch`, pointed at the
worktree path), and hands the worktree path to the agent bridge as its cwd.

The canonical clone's currently-checked-out branch is treated as permanently
locked. The daemon logs a warning at pool init suggesting the canonical clone
be parked on a detached HEAD or a dedicated parking branch if its branch ever
collides with run traffic.

### 4.4 Lease release and cleaning

On run end — completed, failed, cancelled, refused, crashed — the lease is
always released. Release runs, in order:

1. **Salvage (failed/crashed/cancelled runs only):** if the tree is dirty,
   commit everything to the run's work branch as a WIP commit
   (`wip(pidash): salvaged working state from <terminal-status> run <run_id>`)
   and push it, best-effort. A failed run's tree is debugging evidence;
   recycling the desk must never destroy it. Salvage failure is logged loudly
   but does not block release.
2. **Clean:** per `workdir.clean_mode`:
   - `keep-ignored` (default): `git reset --hard` + `git clean -fd`. Drops
     tracked modifications and stray untracked files; **keeps gitignored
     files** — `node_modules`, build caches, `.env` — so the worktree stays
     warm.
   - `allowlist`: `git clean -fdx` everything **except** the work dir's
     `keep_paths` globs (e.g. `["node_modules/**", ".env"]`). Warm where it
     matters, pristine everywhere else.
   - `full`: also `git clean -fdx`. Pristine but cold; the next lease pays
     `setup_command` again (the daemon re-runs it after a `full` clean).

   **Isolation tradeoff, stated plainly:** `keep-ignored` means gitignored
   files written by one run survive into the next run's lease — including
   files a buggy (or prompt-injected) agent planted in `node_modules` or
   build output that a later run might execute. All runs in one work dir are
   the same repo and trust domain, which is why warm-by-default is
   acceptable; work dirs wanting stronger isolation choose `allowlist` (cache
   dirs only) or `full`. This is a per-work-dir knob precisely because the
   right answer is repo-dependent.

3. **Park:** detach HEAD (`git checkout --detach`) so the worktree holds no
   branch lock while idle.
4. **Unlock:** delete the lock file; notify the queue (§5).

Fetches into the shared object database are serialized with one async mutex
per work dir — concurrent fetches are mostly safe in git, but serializing
them is free and removes a class of flaky failures.

### 4.5 Chat sessions

Interactive sessions (`AgentChatSession`) lease a worktree exactly like runs,
holding it for the whole session (open → close), and count against pool
capacity. The lock file records `lease_kind: "session"`. Session leases clean
with the same release path; salvage applies if the session left a dirty tree.

## 5. The local queue

One FIFO queue per work dir, owned by the daemon. The cloud keeps assigning
runs to runners exactly as today (a runner accepts at most one run); the queue
exists because a runner holding an accepted run may still lack a desk —
possible whenever `pool_size <` the number of runners on the work dir, or when
a branch lock blocks the head run.

**Enqueue.** When a `RunnerLoop` receives `Assign` and lease acquisition
fails, it enqueues the run and reports the waiting state to the cloud (§6.1).
It does **not** post `accept` — the accept lifecycle endpoint transitions the
run to `RUNNING` (`apps/api/pi_dash/runner/views/run_endpoints.py`,
`RunAcceptEndpoint`), which would make a desk-less run look live. `accept` is
posted at lease acquisition instead. The instance's status stays `Busy` (it
is single-tenant and committed to this run).

Because instances are single-tenant, a work dir's queue is really an ordered
set of _instances_, each holding its one assigned run. The daemon never holds
more queued runs per work dir than the work dir has runners.

**Dequeue.** Every lease release re-evaluates the queue in FIFO order with
**head-of-line bypass for branch locks only**: a run blocked solely because
its branch is leased elsewhere is skipped (it stays queued); the next run that
can acquire both a worktree and its branch lock starts. Capacity contention
never reorders; only branch contention does. This keeps follow-up runs on the
same branch naturally serialized without letting them stall unrelated work.

**Cancel while queued.** `Cancel { run_id }` from the cloud removes the run
from the queue (no agent process exists yet) and emits `RunCancelled`
immediately. Same for `RemoveRunner`: the removed instance's queued run is
cancelled and reported.

**Rebuild on restart.** The queue is **not** persisted locally. The cloud's
assignment record is the durable truth: when an instance re-opens its session
after a daemon restart, the session-open response carries redelivery of the
instance's outstanding `ASSIGNED` / `WAITING_FOR_WORKTREE` run, if any (§6.3
— this is a new cloud behavior, not today's resume path); the instance
re-attempts lease acquisition and re-enqueues if needed. Queue order after a
restart follows session-open order; exact original order is not guaranteed
and does not matter.

## 6. Cloud touchpoints

Deliberately minimal — the boss's books, not the office's floor plan. These
land in the Django repo (`apps/api/pi_dash/runner/`).

### 6.1 New run status: `WAITING_FOR_WORKTREE`

`AgentRunStatus` (`apps/api/pi_dash/runner/models.py:185`) gains:

```python
WAITING_FOR_WORKTREE = "waiting_for_worktree", "Waiting for Worktree"
```

Non-terminal, sits between `ASSIGNED` and `RUNNING`. The UI renders it as
"queued on <runner's machine> (position N)".

**Transport.** The wire is v4 HTTP long-poll
(`runner/src/cloud/protocol.rs:6`): run lifecycle messages are POSTs to
explicit per-run endpoints, not frames. `RunQueued` is a new lifecycle verb
alongside `accept`/`started`/`complete`:

```
POST /api/v1/runner/runs/<run_id>/queued     body: { queue_position: u32 }
```

Runner-side, `ClientMsg::RunQueued { run_id, queue_position }` dispatches via
`post_run_lifecycle(run_id, "queued", …)` (`runner/src/cloud/http.rs`,
`dispatch_client_msg`), with the same idempotency-key dedupe as its siblings.
Posted on enqueue and again when the position changes (positions only
decrease). The cloud endpoint (`RunQueuedEndpoint`, next to
`RunAcceptEndpoint` in `views/run_endpoints.py`) transitions
`ASSIGNED → WAITING_FOR_WORKTREE` and stores the position; it refuses the
transition from `RUNNING` or any terminal state (late/duplicate queued posts
are acknowledged and dropped, matching the dedupe idiom there).

**Accept moves to lease time.** Today's `accept` endpoint sets the run to
`RUNNING` (`RunAcceptEndpoint`). A queued run therefore must **not** post
`accept` on enqueue. The lifecycle becomes:

```
desk free at Assign:   ASSIGNED ──accept──► RUNNING ──started──► RUNNING …
pool exhausted:        ASSIGNED ──queued──► WAITING_FOR_WORKTREE ──accept──► RUNNING …
```

`accept` is posted at lease acquisition in both paths; `accept` semantics are
unchanged cloud-side.

### 6.2 Status-set and reaper integration

The new status must join the hard-coded status sets, or queued runs fall
through the cloud's bookkeeping:

- **`NON_TERMINAL_STATUSES` and `BUSY_STATUSES`**
  (`apps/api/pi_dash/runner/services/matcher.py:50-66`): both. A queued run
  occupies its single-tenant runner and must block pod deletion exactly like
  `ASSIGNED`.
- **Heartbeat reaper** (`session_service.py`, `_reap_stale_runs`): the reaper
  fails any `BUSY_STATUSES` run assigned before the cutoff that the runner
  does not report as `in_flight_run`. Because instances are single-tenant, an
  instance's queued run _is_ its one outstanding run — the runner reports it
  as `in_flight_run` in the poll status body while it waits. No reaper change
  is needed beyond adding the status to `BUSY_STATUSES`; a daemon that truly
  lost the run (crash without restart) stops reporting it and the reaper
  correctly fails it, exactly as for `RUNNING` today.
- **Cancel path**: `WAITING_FOR_WORKTREE` must be accepted as a cancellable
  state; the daemon gains the dequeue branch (§5).

### 6.3 Redelivery at session open

Today, session open only _resumes_: the runner reports `in_flight_run` and
the cloud builds a resume ack for it
(`apps/api/pi_dash/runner/views/sessions.py:204`). Nothing pushes work the
runner doesn't already know about. That is insufficient here: after a daemon
restart, the local queue is gone and the runner does not know its queued run
exists.

Extend session open: when the opening runner has an outstanding run in
`ASSIGNED` or `WAITING_FOR_WORKTREE` that it did not report as in-flight, the
open response carries a `redeliver` payload mirroring the `Assign` body. The
instance treats it exactly like a fresh `Assign`: attempt a lease, start or
enqueue. (At most one such run exists per runner — single-tenancy.) `RUNNING`
runs the daemon lost remain the reaper's job, unchanged.

### 6.4 Capacity hint for the matcher

In wire v4 there is no Heartbeat frame — runner status rides the long-poll
request body (`{"ack": …, "status": …}`, `runner/src/cloud/http.rs:639`).
The status object gains an optional field:

```json
"free_worktrees": 1    // free desks in this runner's work dir pool
```

The poll handler stores it on the runner row.
`select_runner_in_pod` (`apps/api/pi_dash/runner/services/matcher.py:96`)
uses it as a **preference, not a gate**: among eligible idle runners, prefer
one whose office reports a free desk; fall back to today's oldest-heartbeat
choice. The matcher never refuses to assign because capacity looks full — the
local queue absorbs the overflow, and stale hints therefore cost at most some
queue time, never a stuck run. With agent-targeted routing there is often
exactly one eligible runner anyway; this hint only improves the multi-eligible
case.

## 7. Config shape

```toml
# config.toml

[daemon]
cloud_url = "https://cloud.pidash.so"

[[workdir]]
name = "main-repo"
path = "/home/rich/work/main"          # canonical clone (agents never run here)
pool_size = 2                          # default 2; max concurrent leases
clean_mode = "keep-ignored"            # or "allowlist" or "full" (§4.4)
# keep_paths = ["node_modules/**"]     # allowlist mode only
setup_command = "pnpm install"         # optional; runs once per new worktree,
                                       # and after every `full` clean
# worktrees_dir = "/big-disk/pidash-worktrees/main-repo"   # optional override

[[runner]]
name = "codex-main"
runner_id = "..."
workdir = "main-repo"                  # replaces [runner.workspace].working_dir
[runner.agent]
kind = "codex"

[[runner]]
name = "fable-main"
runner_id = "..."
workdir = "main-repo"                  # same office, different employee
[runner.agent]
kind = "claude_code"
```

**Validation changes** (`runner/src/config/schema.rs`):

- `DuplicateWorkingDir` / `NestedWorkingDir` across **runners** are deleted —
  sharing is the point now.
- The same checks move to **work dirs**: two `[[workdir]]` entries must not
  have equal or nested `path`s (nor equal/nested `worktrees_dir`s).
- Every `runner.workdir` must name an existing `[[workdir]]` (hard error,
  mirroring `MissingProjectSlug`).
- `pool_size` must be ≥ 1; values above 16 warn (a desk cap, not an
  abuse cap — the §16 instance cap of the multi-runner design still governs
  runner count).

**Migration.** Per the multi-runner rollout (§13 there), there are no
production runner users; no compatibility shim is built. On first run with the
new schema, the daemon refuses a config containing `[runner.workspace]` with a
clear message, and `pidash configure --migrate-workdirs` rewrites it: each
distinct `working_dir` becomes a `[[workdir]]` named after the runner (or
dir basename), `pool_size = 2`, and runners get `workdir = <name>` references.

## 8. Crash safety

Locks are on disk so a daemon crash cannot strand desks:

- **Lock files** (`locks/wt-N.lock.json`) are written before checkout and
  deleted after cleaning. They name the lease holder (`run_id` /
  `session_id`, `runner_id`, `acquired_at`).
- **On daemon start**, per work dir: `git worktree prune`; then every lock
  file is stale by definition (no leases survive the process) — each locked
  worktree goes through the full release path (salvage → clean → park →
  unlock) before the pool accepts leases. Salvage on reaped locks uses the
  `run_id` from the lock file for the WIP commit message, preserving the
  pre-crash tree on the run's branch.
- **Worktree corruption** (manual deletion, disk issues): a worktree that
  fails cleaning or `git status` is removed (`git worktree remove --force` +
  prune) and the pool recreates lazily. The pool never repairs in place.

The local queue needs no crash handling — it is rebuilt from cloud state
(§5, §6.3).

## 9. Failure semantics

| Scenario                                                 | Behaviour                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| -------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Pool exhausted on `Assign`                               | Run enqueues; `RunQueued` → cloud shows `WAITING_FOR_WORKTREE`. Never fails for capacity.                                                                                                                                                                                                                                                                                                                                                                                           |
| Run's branch leased by another worktree                  | Run enqueues (or is bypassed at dequeue) until the branch lock frees. Serializes same-branch follow-ups.                                                                                                                                                                                                                                                                                                                                                                            |
| Run's branch checked out in the canonical clone          | Same as above, but the lock may never free — surfaced in `pidash status` and daemon logs ("branch X held by canonical clone; park it").                                                                                                                                                                                                                                                                                                                                             |
| Cancel while queued                                      | Dequeued, `RunCancelled` emitted immediately.                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Run fails / agent crashes with dirty tree                | Salvage WIP commit pushed to the work branch (best-effort), then clean and release. Evidence preserved, desk recycled.                                                                                                                                                                                                                                                                                                                                                              |
| Daemon crash mid-run                                     | On restart: stale locks reaped (salvage + clean), queue rebuilt from cloud re-delivery. In-flight runs are lost as today (run fails cloud-side via lease expiry).                                                                                                                                                                                                                                                                                                                   |
| `setup_command` fails on a new worktree                  | One retry with backoff; on second failure the waiting run fails terminally (`RunFailed`, reason `WorkspaceSetup` — matching today's `supervisor.rs` resolve-failure path) and the **work dir is marked unhealthy**: no new leases, queued runs fail fast with the same reason, state surfaced in `pidash status`/TUI. Cleared by config reload or an explicit retry verb. A broken work dir is a config error, not capacity contention — it must never present as an eternal queue. |
| Two runs race for the last worktree                      | Lease acquisition is serialized per work dir (single async owner task); no race exists.                                                                                                                                                                                                                                                                                                                                                                                             |
| `pool_size` lowered below live worktree count (TUI edit) | Existing leases finish; surplus clean worktrees are removed on release until the count fits.                                                                                                                                                                                                                                                                                                                                                                                        |
| `RemoveRunner` while its run is queued                   | Queued run cancelled and reported; lease never acquired; other queue entries unaffected.                                                                                                                                                                                                                                                                                                                                                                                            |
| Stale `free_worktrees` hint in heartbeat                 | Matcher may pick a deskless runner; run queues locally. Cost is latency, never correctness.                                                                                                                                                                                                                                                                                                                                                                                         |

## 10. CLI / TUI surface

- `pidash workdir add --name main-repo --path ~/work/main [--pool-size N]
[--clean-mode keep-ignored|full] [--setup-command "pnpm install"]` — create
  the entity; `pidash workdir list` / `remove` round it out. `remove` refuses
  while any runner references the work dir.
- `pidash runner add --workdir main-repo ...` — replaces `--working-dir`.
- `pidash status` shows, per work dir: pool occupancy (`2/2 desks busy`),
  queue depth, and per-worktree lease holders.
- **TUI**: the work-dir view exposes `pool_size` (the user-requested knob),
  `clean_mode`, and `setup_command` for editing; a pool panel shows desks,
  lease holders, and the queue. Edits apply live where safe (`pool_size` up:
  immediate; down: drains per §9).

## 11. ADR — queue location: daemon, not cloud

**Considered**: capacity-aware dispatch — the cloud only assigns a run when
the runner can actually start it (free desk), keeping all queueing in the
existing `QUEUED` state and `drain_pod`.

**Why rejected**: whether a run can start depends on facts only the daemon
knows at execution time — desk occupancy _and branch locks_ (a run can be
blocked because its branch is checked out at another desk, or in the
canonical clone). Gating cloud-side would require the cloud to model
branch-level locks, leaking office-interior detail upward, and the daemon
needs deferral machinery for branch contention regardless. Once that exists,
"all desks busy" is just another reason to defer.

**What the cloud-side option got right** is preserved as four guardrails,
all in this design: (1) a cloud-visible `WAITING_FOR_WORKTREE` state with
queue position (§6.1); (2) cancel works while queued (§6.2); (3) the cloud's
assignment record is the durable truth and the local queue is rebuilt from it
(§5, §6.3); (4) desk availability flows back as an assignment _hint_ (§6.4).
The boss doesn't manage desks, but the books always say where every task is,
the boss can recall any task, and the boss glances at the "how busy" sign
before choosing between two equally qualified employees.

## 12. ADR — pool from day one, not per-runner worktrees

**Considered**: one persistent worktree per runner (no pool, no queue) —
strictly simpler, and sufficient for the motivating two-agents-one-repo case.

**Why rejected as the end state**: it re-couples capacity to runner count
(N runners = N worktrees of disk forever, even if only 2 ever run at once),
gives no clean-tree guarantee between runs, and has no answer for chat
sessions and runs contending for the same tree. The pool decouples the
capacity knob (`pool_size`) from the routing concept (runners), which is the
actual point of this design. Accepted cost: lease/queue/cleaning machinery
that per-runner worktrees would not need — judged worth building once,
correctly, rather than migrating later.

## 13. Deferred

- **Idle-worktree eviction**: removing clean worktrees after a TTL to reclaim
  disk. Lazy creation bounds the waste at "high-water mark of concurrent
  leases"; eviction can come later without protocol changes.
- **Sparse checkout / partial clone** (`--filter=blob:none`) for very large
  repos: pure work-dir-level optimizations, addable behind `[[workdir]]`
  flags later.
- **Priority or per-agent-kind queue ordering**: FIFO with branch bypass
  until real usage says otherwise.
- **Worktree-per-run for parallel runs inside one instance**: would lift
  single-tenancy; out of scope (multi-runner design §1 still governs).

## 14. Files most affected

### Runner (`runner/`)

| File                            | Change                                                                                                                             |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `src/workspace/pool.rs` (new)   | Pool: lease/release, lazy creation, cleaning, salvage, branch locks, lock files, crash reaping, per-work-dir queue.                |
| `src/workspace/resolve.rs`      | Resolves the **canonical clone** for a work dir instead of a runner's dir; unchanged clone-if-empty semantics.                     |
| `src/workspace/git.rs`          | Adds `worktree_add/remove/prune`, `detach_head`, `reset_clean(mode)`, `salvage_wip(run_id)`; fetch mutex per work dir.             |
| `src/config/schema.rs`          | `[[workdir]]` entity; runner `workdir` reference; validation flip (§7); `pool_size`/`clean_mode`/`setup_command` fields.           |
| `src/daemon/supervisor.rs`      | `RunnerLoop` acquires a lease before spawning the agent bridge; enqueue path; dequeue-on-release wakeups; cancel-dequeue.          |
| `src/cloud/protocol.rs`         | `ClientMsg::RunQueued { run_id, queue_position }` variant.                                                                         |
| `src/cloud/http.rs`             | Dispatch `RunQueued` via `post_run_lifecycle(…, "queued", …)`; `free_worktrees` in the poll status body; 404 feature-detect (§15). |
| `src/cli/{runner,configure}.rs` | `pidash workdir` verbs; `runner add --workdir`; `--migrate-workdirs`.                                                              |
| `src/ipc/protocol.rs`           | `StatusSnapshot` gains per-work-dir pool/queue info.                                                                               |
| `src/tui/*`                     | Work-dir view: pool panel, `pool_size` editing.                                                                                    |

### Cloud (`apps/api/pi_dash/runner/`)

| File                          | Change                                                                                                              |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `models.py`                   | `AgentRunStatus.WAITING_FOR_WORKTREE`; queue-position field on `AgentRun` (display only).                           |
| `views/run_endpoints.py`      | New `RunQueuedEndpoint` (lifecycle verb `queued`, `ASSIGNED → WAITING_FOR_WORKTREE`).                               |
| `views/sessions.py`           | Session open gains the `redeliver` payload for outstanding unreported runs (§6.3).                                  |
| `services/matcher.py`         | Add status to `NON_TERMINAL_STATUSES` + `BUSY_STATUSES` (`:50-66`); free-desk preference in `select_runner_in_pod`. |
| `services/session_service.py` | Poll status accepts `free_worktrees`; reaper covers the new status via `BUSY_STATUSES` (no logic change).           |
| cancel path                   | `WAITING_FOR_WORKTREE` is cancellable.                                                                              |

### Web / types (pnpm workspace)

| File                                                               | Change                                                       |
| ------------------------------------------------------------------ | ------------------------------------------------------------ |
| `packages/types/src/runner.ts:99`                                  | Add `"waiting_for_worktree"` to the `TAgentRunStatus` union. |
| `apps/web/core/components/issues/issue-detail/agent-status.tsx:38` | Add to `ACTIVE_RUN_STATUSES`; render label + queue position. |
| `apps/web/app/(all)/[workspaceSlug]/runners/runs/page.tsx`         | Status filter/label maps gain the new value.                 |
| mobile vendored runner types (if present)                          | Mirror the union change.                                     |

## 15. Rollout

1. **Runner repo**: pool module + config schema + lease-before-run, behind the
   new config shape (old shape refuses with the migration hint, §7). The
   daemon stays functional against an unmodified cloud via explicit feature
   detection — on HTTP there is no "frame dropping": a POST to the unknown
   `…/queued` endpoint returns 404. The runner treats a 404 from `queued` as
   "cloud predates this feature": log once per session, stop posting `queued`,
   and **post `accept` on enqueue instead** (legacy behavior — the run shows
   `RUNNING` while it waits, exactly as an old cloud expects; degraded
   visibility, no stuck states). Unknown poll-status fields
   (`free_worktrees`) are ignored by old clouds — additive and safe.
2. **Cloud repo**: new status + `RunQueuedEndpoint` + status-set additions
   (§6.2) + session-open redelivery (§6.3) + cancellable state. One deploy —
   the status must not land without the matcher/reaper set additions, or
   queued runs are invisible to the busy/reaper logic (§6.2).
3. **Matcher hint + web UI/types** last: pure polish, no correctness
   dependency (the TS union addition ships with step 2's API change, since
   the serializer starts emitting the new value then).

Companion work (separate doc): **agent-aware routing** — runners report
`agent.kind`/model at registration, issues carry an agent preference, matcher
filters on it. Together with this design it completes "route this issue to
codex, that one to fable, same repo, one machine."
