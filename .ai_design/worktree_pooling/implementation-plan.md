# Implementation Plan — Worktree Pooling

Phased rollout of `design.md`. Each phase is independently mergeable and
leaves the system in a working state. Phases 1–3 are runner-side and run
end-to-end against an **unmodified cloud** (the design's §15 feature-detect
guarantee is built in from the start, not retrofitted). Phases 4–5 are
cloud-side. Phases 6–7 join the two. Phase 8 is surface polish.

Per the multi-runner rollout (`../n_runners_in_same_machine/design.md` §13)
there are no production runner users; the old `[runner.workspace]` config
shape gets a migration command, not a compatibility shim.

---

## Phase 1 — Runner: `[[workdir]]` config entity (execution unchanged)

**Goal**: the work dir exists as a named, shared entity in config; runners
reference it. Agents still execute directly in the canonical clone, exactly as
today — this phase is config plumbing only.

- `runner/src/config/schema.rs`:
  - New `WorkdirConfig { name, path, pool_size (default 2), clean_mode
(default keep-ignored), keep_paths, setup_command, worktrees_dir }`;
    `Config.workdirs: Vec<WorkdirConfig>`.
  - `RunnerConfig` drops `workspace: WorkspaceSection`, gains
    `workdir: String` (reference by name).
  - Validation flip (design §7): delete the runner-pair
    `DuplicateWorkingDir` / `NestedWorkingDir` checks (`schema.rs:452-476`);
    add the same equal/nested-path checks **across `[[workdir]]` entries**
    (both `path` and `worktrees_dir`); hard error on a `runner.workdir` that
    names no `[[workdir]]` (mirror `MissingProjectSlug`); duplicate workdir
    names; `pool_size ≥ 1`, warn above 16.
  - **Temporary guard, removed in phase 3**: at most one runner per workdir.
    Until the pool exists, two runners sharing a canonical clone is exactly
    the corruption hazard the old validation prevented. The error message
    says the cap lifts with pooling.
- `pidash configure --migrate-workdirs` (`runner/src/cli/configure.rs`):
  rewrites an old-shape config — each distinct `working_dir` becomes a
  `[[workdir]]` (named after the runner or dir basename), runners get
  `workdir = <name>`. A config containing `[runner.workspace]` refuses to
  start with a message naming this command.
- CLI: `pidash workdir add|list|remove` (config-file writes + IPC reload);
  `pidash runner add --workdir <name>` replaces `--working-dir`. `workdir
remove` refuses while any runner references it.
- `runner/src/daemon/supervisor.rs` `handle_assign` (~`:2272`): resolve
  `runner.workdir → workdir.path` and proceed exactly as today
  (`workspace::resolve` into the canonical clone).

**Done when**: fresh install with the new shape runs a real assignment
end-to-end in the canonical clone; `--migrate-workdirs` round-trips an
old-shape config; two workdirs with nested paths refuse startup with a clear
error; two runners on one workdir refuse startup (temporary guard).

## Phase 2 — Runner: worktree primitives + pool library (unwired)

**Goal**: `WorktreePool` exists as a fully-tested library. Nothing in the
daemon calls it yet.

- `runner/src/workspace/git.rs` additions (all shelling out via the existing
  `git_output` helper, branch names through `validate_branch_name`):
  - `worktree_add(repo, path)` (`git worktree add --detach`),
    `worktree_remove(repo, path, force)`, `worktree_prune(repo)`.
  - `detach_head(worktree)` (`git checkout --detach`).
  - `reset_clean(worktree, mode, keep_paths)` — `reset --hard` plus
    `clean -fd` / `clean -fdx` with `--exclude` globs for `allowlist`.
  - `salvage_wip(worktree, branch, holder_id)` — stage-all, WIP commit with
    the design's message shape, best-effort push.
  - `checked_out_branches(repo) -> map<branch, path>` — parse
    `git worktree list --porcelain` (covers the canonical clone too); this is
    the branch-lock source of truth.
- `runner/src/workspace/pool.rs` (new) — one owner task per workdir; all
  lease/queue state lives inside it, so acquisition is serialized by
  construction (design §9 "no race exists"):
  - `acquire(LeaseRequest { kind: Run|Session, holder_id, branch }) →
oneshot<LeaseGrant>` — grants immediately, or parks the request in the
    FIFO queue (granted later via the oneshot; this _is_ the local queue).
  - Grant path: free worktree (or lazy `worktree_add` under `pool_size` +
    `setup_command` with one retry/backoff), branch-lock check via
    `checked_out_branches`, write `locks/wt-N.lock.json`, checkout branch.
  - `release(lease, outcome)`: salvage (non-success outcomes, dirty tree
    only) → clean per `clean_mode` → `detach_head` → unlock → re-evaluate the
    queue FIFO with branch-lock-only head-of-line bypass (design §5).
  - `cancel(holder_id)`: remove a parked request from the queue (resolves the
    oneshot with `Cancelled`).
  - `PoolHealth::Unhealthy { reason }` after the bounded `setup_command`
    retry fails: new acquires and parked requests fail fast (design §9);
    cleared by config reload or an explicit retry verb.
  - Startup reap: `worktree_prune`, then every lock file (stale by
    definition) goes through the full release path; corrupted worktrees are
    force-removed and recreated lazily (design §8).
  - Per-workdir async fetch mutex, exposed for the resolve/fetch paths.
  - `snapshot()` for IPC/TUI: per-worktree lease holder, queue contents,
    health.
- Tests (tempdir git repos, like `resolve.rs`'s existing tests): lease/release
  round-trip per clean mode; lazy growth stops at cap; branch-lock bypass
  ordering; cancel-while-parked; salvage on failure outcome; crash reap from
  a hand-written lock file; unhealthy after two setup failures; corrupted
  worktree replaced.

**Done when**: `cargo test` covers the matrix above green; no daemon behavior
change.

## Phase 3 — Runner: wire the pool into the supervisor (legacy reporting)

**Goal**: runs and chat sessions execute in pooled worktrees; N runners per
workdir works; queueing works — all against an **unmodified cloud** using the
design §15 legacy fallback (post `accept` on enqueue; the run shows `RUNNING`
while it waits).

- Supervisor startup: build one `WorktreePool` per configured workdir
  (initialization = canonical-clone resolve + reap, design §4.2); log the
  canonical clone's checked-out branch warning (design §4.3).
- `handle_assign`: replace the direct `workspace::resolve` call with
  `pool.acquire(...)`. Immediate grant → post `accept` → spawn the agent
  bridge with the worktree path as cwd. Parked → post `accept` (legacy mode;
  flips in phase 6) and await the oneshot; on grant, proceed.
- **Reaper protection**: while a run is parked, the instance reports it as
  `in_flight_run` in the poll status body. Without this, the cloud's
  heartbeat reaper fails any queued run older than the assignment grace
  (design §6.2). This must ship in the same PR as the queue.
- Every terminal path (`RunCompleted` / `RunFailed` / `RunCancelled` /
  refusal / bridge crash) calls `pool.release(lease, outcome)`; the
  `Cancel` handler gains the dequeue branch (`pool.cancel(run_id)` → emit
  `RunCancelled` immediately).
- Unhealthy workdir: parked runs fail with `RunFailed { reason:
WorkspaceSetup }` (same `FailureReason` as today's resolve failures,
  `supervisor.rs:2288`).
- Chat sessions (`runner_direct_chat` path) acquire/release leases with
  `lease_kind: Session`, held open → close.
- Remove phase 1's temporary one-runner-per-workdir guard.

**Done when**: two runners (codex + claude_code) on one workdir with
`pool_size = 1` run two concurrent assignments — the second waits, then runs,
both produce pushed branches; `kill -9` mid-run, restart: stale lock reaped,
dirty tree salvaged to the run's branch; cancel of a parked run reports
`RunCancelled` without ever leasing; all against an unmodified cloud.

## Phase 4 — Cloud: `WAITING_FOR_WORKTREE` + `queued` endpoint + status sets

**Goal**: the waiting state is first-class on the boss's books.

- `apps/api/pi_dash/runner/models.py`: `AgentRunStatus.WAITING_FOR_WORKTREE`;
  nullable `queue_position` small-int on `AgentRun` (display only) +
  migration.
- `views/run_endpoints.py`: `RunQueuedEndpoint` (URL verb `queued`, wired in
  the runner urlconf next to `accept`/`started`): same `_RunEndpointBase` +
  dedupe idiom; transitions `ASSIGNED → WAITING_FOR_WORKTREE` and updates
  `queue_position` (also accepts `WAITING → WAITING` position refreshes);
  acknowledges-and-drops from `RUNNING` or terminal states.
- `services/matcher.py:50-66`: add the status to both `NON_TERMINAL_STATUSES`
  and `BUSY_STATUSES`.
- Cancel path: the user-facing cancel view treats `WAITING_FOR_WORKTREE` as
  cancellable (delivers `Cancel` to the runner as for `RUNNING`).
- Serializers expose the new status + `queue_position`;
  `packages/types/src/runner.ts:99` union + status maps gain the value in the
  same PR (the API starts emitting it now — web must not choke on it; full
  rendering is phase 8).
- Tests: endpoint transition matrix; queued run blocks pod deletion; reaper
  kills an _unreported_ queued run and spares a _reported_ one (this is the
  contract phase 3's reporting depends on); cancel from waiting.

**Done when**: contract tests pass; a phase-3 runner pointed at this cloud
gets 200 from `…/queued` (used in phase 6) and nothing else changes for it.

## Phase 5 — Cloud: session-open redelivery

**Goal**: a restarted daemon learns about the queued run it forgot.

- `views/sessions.py` (session open, after the resume-ack step at `:204`):
  query the opening runner's outstanding run in `ASSIGNED` /
  `WAITING_FOR_WORKTREE` that it did **not** report as `in_flight_run`; if
  one exists (at most one — single-tenancy), include a `redeliver` payload in
  the open response mirroring the `Assign` envelope body (built by the same
  helper the matcher uses, `services/matcher.py` assign-envelope
  construction).
- Tests: waiting run + reopen with no `in_flight` → `redeliver` present;
  reopen _with_ the run reported → resume ack only, no redeliver; `RUNNING`
  runs never redeliver (reaper's job).

**Done when**: integration test proves both paths; old runners (which ignore
unknown response fields) are unaffected.

## Phase 6 — Runner: first-class queued reporting + redelivery handling

**Goal**: the legacy fallback becomes the fallback; the real protocol becomes
the default.

- `runner/src/cloud/protocol.rs`: `ClientMsg::RunQueued { run_id,
queue_position }`.
- `runner/src/cloud/http.rs` `dispatch_client_msg`: route it via
  `post_run_lifecycle(run_id, "queued", …)`. **Feature detection**: a 404
  from the `queued` verb sets a per-session "cloud predates queued" flag —
  log once, stop posting `queued`, revert to accept-on-enqueue (phase 3
  behavior). Any other error follows the normal lifecycle retry path.
- Enqueue path flips: post `queued` instead of `accept`; post `accept` at
  lease grant (design §6.1 lifecycle diagram); post position refreshes as the
  queue drains (positions only decrease).
- Session open: parse the `redeliver` payload and feed it into the
  `handle_assign` path verbatim.
- Fake-cloud test double (`runner/tests/…`) gains the `queued` endpoint and
  a 404 mode to exercise both branches.

**Done when**: against a phase-5 cloud, a parked run shows
`WAITING_FOR_WORKTREE` with a position in the API, flips to `RUNNING` at
lease grant, and survives a daemon restart via redelivery (re-queued, then
runs); against the 404 fake, behavior is byte-identical to phase 3.

## Phase 7 — Capacity hint for the matcher

**Goal**: the boss glances at the "how busy" sign when choosing between two
eligible employees.

- Runner: the long-poll status body (`http.rs:639`) gains
  `free_worktrees: u32` — free desks in _this instance's_ workdir pool at
  poll time.
- Cloud: poll handler persists it (nullable int on `Runner` + migration);
  `select_runner_in_pod` (`matcher.py:96`) orders eligible runners by
  free-desk-reported first, then today's oldest-heartbeat. Preference, never
  a gate — `None` (old runner) sorts with the no-free-desk group.
- Tests: two eligible runners, one reporting a free desk → it wins; both
  `None` → today's ordering unchanged.

**Done when**: matcher test matrix passes; assignment latency for the
two-runner-one-desk topology visibly improves in the integration test (first
assign goes to the runner whose pool has the free desk).

## Phase 8 — CLI / TUI / Web surface

**Goal**: the pool is operable and observable (design §10).

- `runner/src/ipc/protocol.rs`: `StatusSnapshot` gains per-workdir
  `{ pool: [{worktree, lease_holder, branch}], queue: [run_id…], health }`
  from `pool.snapshot()`; bump IPC version.
- `pidash status`: per-workdir occupancy line (`2/2 desks busy, 1 queued`),
  unhealthy reason, canonical-clone branch warning.
- `pidash workdir retry <name>`: clears `Unhealthy` and re-attempts setup.
- TUI: workdir view with pool panel (desks, lease holders, queue) and editing
  for `pool_size` (the user-requested knob), `clean_mode`, `setup_command`.
  `pool_size` up applies live; down drains on release (design §9).
- `pidash doctor`: per-workdir checks — canonical clone resolvable, branch
  parking advice, worktree dir writable, disk headroom, setup_command
  exit status.
- Web: `agent-status.tsx:38` adds the status to `ACTIVE_RUN_STATUSES` with a
  "queued on machine (position N)" label; runs page filter/label maps;
  mobile vendored types mirrored.

**Done when**: a user can watch a run wait and start from both `pidash tui`
and the web run view, and resize the pool from the TUI without a restart.

---

## Cross-cutting work

- **Logging**: pool operations log under a per-workdir tracing span
  (`workdir`, `worktree`, `lease_kind`, `holder_id`); salvage results always
  logged at `warn` or above.
- **Metrics**: gauges for pool occupancy, queue depth, workdir health; a
  histogram for lease wait time (queue latency is the number that says
  whether `pool_size` defaults are right).
- **Contract tests**: the v4 wire contract suite gains the `queued` verb,
  the `redeliver` open-response field, and `free_worktrees` in poll status.
- **Docs**: `pidash runner add` help text, README quick-start, and the
  install-script template move from `--working-dir` to `--workdir`.

## Risk register

| Risk                                                                             | Likelihood | Mitigation                                                                                                                                                                                    |
| -------------------------------------------------------------------------------- | ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Reaper kills parked runs (runner forgets to report `in_flight_run` while queued) | Medium     | Reporting ships in phase 3, _before_ any cloud change; phase 4 adds the contract test that fails if either side regresses.                                                                    |
| Branch-lock starvation: canonical clone permanently holds a branch runs need     | Medium     | Init warning + `pidash status` surfacing + doctor check (phases 3/8). The failure mode is a visible never-draining queue entry, not corruption.                                               |
| Salvage push fails (auth, network) and the WIP commit never leaves the machine   | Medium     | The commit still exists on the local branch ref in the shared object database — `git log` in the canonical clone finds it. Push is best-effort by design; the log line names the commit hash. |
| Flaky `setup_command` (network installs) trips workdir-unhealthy too eagerly     | Medium     | One retry with backoff before unhealthy; `pidash workdir retry` is one command; health is per-workdir, never daemon-wide.                                                                     |
| Concurrent git ops against the shared object DB (fetch vs auto-gc vs checkout)   | Low        | Fetch mutex per workdir (phase 2); set `gc.auto = 0` on the canonical clone at pool init and let doctor recommend manual gc.                                                                  |
| Cancel races lease grant (cancel arrives the instant the oneshot fires)          | Low        | All transitions happen inside the single pool owner task; a granted lease that races cancel follows the normal running-cancel path (design §6.2 note). Property test in phase 2.              |
| Disk growth from N worktrees of a huge repo                                      | Low        | Lazy creation bounds at the concurrency high-water mark; `keep-ignored` reuses installs; eviction is the designed follow-up (design §13).                                                     |
| Old-shape config on a dev machine fails to start after upgrade                   | Low        | Refusal message names `pidash configure --migrate-workdirs`; migration is lossless and tested in phase 1.                                                                                     |

## Estimated scope

| Phase | PRs | Notes                                                                               |
| ----- | --- | ----------------------------------------------------------------------------------- |
| 1     | 1–2 | Config/validation churn is wide but mechanical; migration command needs care.       |
| 2     | 1–2 | The big one by line count, but pure library + tests; no integration risk.           |
| 3     | 1–2 | The risky one — supervisor lifecycle paths. Reaper-protection must land atomically. |
| 4     | 1   | Focused Django change; the contract tests are the bulk.                             |
| 5     | 1   | Small, surgical.                                                                    |
| 6     | 1   | Protocol flip + feature detection; fake-cloud work dominates.                       |
| 7     | 1   | Small on both sides.                                                                |
| 8     | 2–3 | TUI editing + web rendering are the long pole; independently shippable pieces.      |

Total: ~9–13 PRs. Phases 1→2→3 are strictly sequential (runner repo);
4→5 sequential (cloud repo) and can start any time after the design lands;
6 needs 3+5; 7 needs 6; 8 trails everything and can be parallelized.
