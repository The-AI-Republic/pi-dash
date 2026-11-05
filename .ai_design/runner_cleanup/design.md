# Runner Cleanup: agents clean up after themselves

**Status:** v3, revised after two review rounds by Claude and Codex (2026-11-05) **Scope:** The `pidash` runner (`runner/`). Every run leaves the dev machine as it found it: no leftover processes it started, no stray temp files, bounded transcripts and logs, and visible warnings when the disk is low. Runner-only; no server or protocol change.

---

## 1. Problem

Facts from `origin/main` (fc32150e7; re-checked at ff51dc51f):

| Area        | Today                                                                                                                                                                                                                                                                                                                                                                   | Code                                                                                                |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Spawning    | Every agent kind spawns through `util::shell::login_shell_command[_with_env]` (Unix: `bash -ilc '<script>; exec "$@"'`, so the held pid is the agent itself; Windows: direct spawn). No `setsid`, process group, `killpg` or Job Object anywhere.                                                                                                                       | `util/shell.rs:164,231`                                                                             |
| Stopping    | `shutdown(grace)` closes stdin, sends a `Graceful` request that is a no-op, waits `grace` (5 s for runs), then `start_kill` on the agent pid only. Anything the agent started (dev servers, test workers, build daemons) survives.                                                                                                                                      | `claude_code/process.rs:177-220` and the other bridges                                              |
| Drop leak   | claude_code, codex and grok keep the `Child` in a `wait_task` that, when `kill_tx` closes, waits for the process to exit on its own: a dropped bridge leaves the agent running. cursor_agent, muse_code and openclaw already kill on channel close.                                                                                                                     | `claude_code/process.rs:~205-232`; `codex/app_server.rs:150-172`; `cursor_agent/process.rs:158-168` |
| Error path  | `pump_events(...).await?` returns on error without calling `shutdown`.                                                                                                                                                                                                                                                                                                  | `daemon/supervisor.rs:3424-3428`                                                                    |
| Daemon exit | SIGTERM/SIGINT start daemon shutdown (`util/signal.rs:7-19`): runs are journaled as `RunFailed{DaemonRestart}`, then tasks are aborted. Agents die only via interrupt or runtime teardown (`kill_on_drop`, leader only). On macOS, launchd also kills the job's process group on daemon exit (`AbandonProcessGroup` defaults to false); agents are in that group today. | `supervisor.rs:245-312`                                                                             |
| Temp files  | Agents write scratch files to the system temp dir; nothing scopes or deletes them.                                                                                                                                                                                                                                                                                      | none                                                                                                |
| Transcripts | `runners/<id>/history/runs/<run_id>.jsonl` is append-only and never deleted; index eviction (cap 500) does not delete files.                                                                                                                                                                                                                                            | `history/jsonl.rs:52`; `history/index.rs:39-52`                                                     |
| Logs        | `log_retention_days` (default 14) is never read. launchd's `runner.out.log`/`runner.err.log` are never rotated.                                                                                                                                                                                                                                                         | `config/schema.rs:66-67,96`; `service/launchd.rs:104-105`                                           |
| Old folders | Pool-era `worktrees/<name>/` and `runners/<id>/chat-worktree/` are only reported, on purpose: they "may hold uncommitted agent work".                                                                                                                                                                                                                                   | `config/migrate.rs:33-36`                                                                           |
| Disk        | No free-space check anywhere.                                                                                                                                                                                                                                                                                                                                           | none                                                                                                |

## 2. Goals

1. **A run cleans up the processes it started.** On every end of a run (finish, error, cancel, watchdog timeout, daemon shutdown, runner removal, bridge drop, daemon crash then restart), processes still in the run's process group are terminated. Processes that deliberately left the group are reported.
2. **A run owns its scratch space**, deleted when the run ends.
3. **Bounded growth** of transcripts and service logs.
4. **Visible disk pressure** (warnings and `pidash doctor`), without changing dispatch.
5. **Safe by construction:** signal only process groups this daemon created and recorded; delete only paths this daemon created; never touch a working tree's contents or any folder that may hold uncommitted work.

## 3. Non-goals (phase 1)

- Killing processes that left the run's process group (`setsid`, Node `detached: true`, shared build daemons). Phase 1 **reports** them; killing is opt-in (4.4).
- Docker containers started by an agent: they run under dockerd/the VM, outside both mechanisms.
- Disk- or memory-based admission (declining work), memory limits, build-cache pruning, git maintenance, deleting pool-era or orphaned runner folders. Phase 2 (§9).
- Any protocol or server change.

## 4. Design

### 4.1 Process ownership

**New session per agent (Unix).** In the shared spawn helper, call `setsid()` in `pre_exec` (via `nix`). The agent becomes leader of a new session and process group (pgid = pid). Not `process_group(0)`: an interactive `bash -i` in a background group of a terminal-attached session stops itself with SIGTTIN before reaching `exec` (reproduced under a pty), which would hang foreground `pidash __run` and terminal test runs. With `setsid` there is no controlling terminal; bash prints the two "no job control" lines that `is_benign_login_shell_warning` already filters. Interrupts are unaffected: they already target the leader pid, which is the agent because of `exec`.

**`util/proctree.rs`:**

```
ProcessTree { record_id, pgid, leader_start_time, state }   // one owner; finish and drop serialized
  before spawn: write a provisional record data_dir/proc/<record_id>.json
                {record_id, instance_id, runner_id, run_or_chat_id, tag, scratch}
                atomically (tmp file, fsync, rename); record_id is a fresh UUID
  after spawn:  fill in pgid and leader start time (atomic rewrite)
  finish(grace):  // REPLACES each bridge's shutdown; no unbounded waits anywhere
      1. graceful: close stdin, wait for the leader up to `grace` (today's behaviour, now bounded)
      2. group still has members (kill(-pgid,0) == 0): SIGTERM the group, wait ≤ 3 s
      3. still members: SIGKILL the group, wait ≤ 2 s
      4. group confirmed gone (ESRCH): mark Gone, disarm all further signalling, delete the record
         otherwise (timeout, EPERM): keep the record for recovery, warn
  drop: if state != Gone, synchronous killpg(SIGKILL) backstop (no waiting)
```

Rules: a pgid is signalled only while this owner has observed it alive and not yet observed it gone; after ESRCH it is never signalled again. EPERM is "could not clean", never "empty", unless every member of the group is a zombie: macOS returns EPERM from `kill(-pgid, …)` while only unreaped zombies remain (Linux delivers instead), so on EPERM the members are listed and a zombie-only group counts as gone (zombies cannot run again; our own leader is reaped by its bridge, orphans by init/launchd). A group that only holds zombies is treated as empty on both platforms. On macOS the members are listed with `sysctl(KERN_PROC_PGRP)`, whose `kinfo_proc` entries include zombies; a member counts as a zombie when `p_stat == SZOMB` or when `P_WEXIT` is set in `p_flag` (right after SIGKILL macOS shows the process as `SRUN` with `P_WEXIT` before it turns into `SZOMB`, and `kill(-pgid, …)` is already EPERM then); `proc_pidinfo` returns nothing for a zombie and is used only for live members' details. Uninterruptible processes end a wait at its bound with a warning. `pre_exec` contains only the `setsid` syscall and errno conversion (no allocation, logging or locks).

**Cleanup before completion is published.** Today `pump_events` publishes `RunCompleted`/`RunFailed` (`supervisor.rs:3975,4014`) before the caller's shutdown (`:3428`). Reorder so the run's tree is finished first and completion is published after, on every path: normal finish, the `pump_events` error path, spawn/startup failure, cancel, watchdog timeout, runner removal. claude_code, codex and grok change their `None` branch to call `finish(2 s)`.

**Daemon shutdown:** stop accepting new spawns (each spawn holds an admission from writing its record until its leader is recorded; shutdown waits for admitted spawns before it stops accepting, so none is missed by the snapshot or finished before it is armed), snapshot live trees, `finish` them in parallel, then journal/publish `RunFailed{DaemonRestart}`; all inside launchd's 20 s `ExitTimeOut` (finish ≤ 10 s, publish ≤ 5 s).

**Crash recovery (replaces launchd's group kill, which `setsid` gives up):** at daemon start, after taking the exclusive instance lock (below), for each record whose `instance_id` is ours:

- Probe the group with `kill(-pgid, 0)`. A group with any live member cannot have its pgid reused, but the record could be older than a reuse. Clean it only if it is proven ours:
  - **Tag (Linux):** at least one live member carries our `PIDASH_PROC_TAG` (read via the same env reader as 4.4).
  - **Leader identity:** a process with pid == pgid exists and its start time equals the recorded `leader_start_time_ms` (within 2 s, for Linux `btime` jitter), and no member was readable without our tag (readable negative tag evidence outweighs the approximate match, since rapid pid reuse can land inside the tolerance). A process with pid == pgid and any other start time means the pgid was reused: do not signal, resolve the record.
  - **Leaderless lifetime window:** no process with pid == pgid, and every member started ≥ the recorded leader start and ≤ the owning daemon's last-alive mark. The leader start alone is not a proof: a group created after the crash, whose leader got pid == the stale pgid, also has every member started after our leader. The instance-lock holder rewrites `data_dir/proc/alive` (atomic) every 5 s, so after a crash the unprovable window (members started after the last mark) is about 5 s; recovery reads it before starting the new heartbeat, and the first pass that sees a record stamps that mark on it (`owner_alive_until_ms`) before any signal, so a pass cut short mid-cleanup still leaves it, so a later daemon's mark never widens the window.
  - Anything else (no mark, members after the mark, readable members without our tag) cannot be proven: do not signal, keep the record. A member older than the recorded leader means reuse: resolve without signalling.
- **macOS has no environment proof.** `KERN_PROCARGS2` for another process (same uid, even an unsigned binary) succeeds but returns only argc, the exec path and argv, with no environment; `ps -E` shows none either. So on macOS ownership rests on the leader identity and the lifetime window only, and an empty environment is treated as unreadable, never as "no tag".
- The same proof guards a live tree after its leader was reaped (chat-turn processes, leader exits first): before every later signal the group is re-proven ours, with the leader known dead (any pid == pgid is a reuser) and the window ending now.
- Cleaning in recovery means signal-and-poll only (the leader is not our child, so no `wait()`): SIGTERM, poll ≤ 3 s, SIGKILL, poll ≤ 2 s. Before the SIGKILL the group is re-proven ours (the same proof, or every live member is one proven before the SIGTERM, matched by pid and start time); a group that emptied and whose pgid was reused while polling is never escalated on.
- Delete the record only once the group is confirmed gone; keep unresolved records and retry at the next start and from the janitor.
- A provisional record without a pgid (crash between record and spawn) is resolved by the tag scan alone, then deleted. On macOS no tag scan is possible: nothing is signalled and the record is dropped. The usual case is a leaderless group (the agent exits on stdin EOF when the daemon dies, its children remain), which this handles.

**Instance identity and lock:** `data_dir/instance_id` (random, created atomically with `O_CREAT|O_EXCL`), and an exclusive advisory lock on `data_dir/instance.lock` held for the daemon's lifetime. A second daemon on the same data dir does not run recovery or the janitor (it logs and continues without them, or refuses to start if the existing single-daemon check already does so). Different data dirs have different ids and never touch each other's records.

**Windows (separate PR, §8):** assign the child to a Job Object right after spawn (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`); `finish` = graceful step, then `TerminateJobObject`. No `CTRL_BREAK_EVENT` (scheduled tasks usually have no shared console). tokio cannot spawn suspended, so a grandchild created in the first microseconds can escape; documented, or closed later with `PROC_THREAD_ATTRIBUTE_JOB_LIST`. Until that PR, Windows keeps today's behaviour behind the same API.

### 4.2 Per-run scratch folder

At run start, create a short private folder `<system temp>/pd-<8 hex>` (mode 0700, `mkdtemp`-style), record its path in the run state and the proc record, and pass it to the agent as `PIDASH_TMPDIR`. `SHELL_SCRIPT` re-exports `TMPDIR`, `TMP` and `TEMP` from `PIDASH_TMPDIR` after rc files run (the same way `CODEX_HOME` is re-exported), so user rc files cannot override it. Short path because Unix socket paths are limited to 104 bytes on macOS (tmux, ssh ControlMaster, gpg-agent, Python multiprocessing, watchman all put sockets in `$TMPDIR`), and no spaces (the macOS data dir has "Application Support").

At run end, after `finish` confirms the group gone, delete the folder with the path guard (4.5) unless the escapee report (4.4) found live tagged processes; in that case leave it, mark it in the record, and let the janitor delete it once no tagged process remains (or after 7 days). Crash recovery and the janitor delete scratch folders of resolved records. On Windows, `TMP`/`TEMP` are set directly in the agent environment (no shell script), the same way `AgentEnv::apply` sets `CODEX_HOME`. Children started by rc files before the re-export still see the user's `TMPDIR`; documented.

**Chat:** one scratch folder and one process tree per chat runtime. Processes stay alive between turns of a live runtime, and are finished (and the scratch deleted) whenever the runtime ends: `Release` (sent before every Assign and on session swaps, `supervisor.rs:1493,1669,1746,2221-2242`), the 5-minute idle timeout (`:2098`), Close, runner removal and daemon shutdown. So a dev server a chat agent starts runs until the chat goes idle or a run needs the working dir. Documented behaviour. One-shot engines (Cursor, MuseCode, OpenClaw) exit after every turn; a bridge whose leader has exited is retired into the runtime, not finished, and every retired bridge is finished with the current one when the runtime ends.

### 4.3 Process tag

Add `PIDASH_PROC_TAG=<instance_id>:<run|chat>:<id>` to the agent environment. `PIDASH_RUN_ID` is unchanged (the CLI sends it as `X-Pi-Dash-Run-Id`, which the server requires to be a UUID; chat keeps not setting it).

### 4.4 Escapee report

After `finish` for a run, and at daemon start for this instance's stale records, list same-uid processes whose environment contains this instance's `PIDASH_PROC_TAG` (Linux `/proc/<pid>/environ`; unreadable processes skipped). **Linux only; macOS reports nothing:** macOS does not expose other processes' environments (`KERN_PROCARGS2` returns argv only, see 4.1), so environment-based escapee detection cannot work there. Log one warning per run with pid and executable name (not full command lines, which may hold secrets), and keep the latest list for `pidash doctor`.

Killing escapees is **off by default** (`[cleanup] kill_escapees = false`). When on: re-read the process start time and tag immediately before SIGTERM and before SIGKILL; skip an allowlist of shared build daemons (gradle, bazel, sccache, watchman, turbo, kotlin daemon) by executable name. Best effort: environments can be cleared, and a process can fork after enumeration.

### 4.5 Janitor and path guard

A daemon-wide task beside `machine_handle` / `triggers_handle` (`supervisor.rs:225-244`), aborted with them; runs 5 minutes after start, then every 24 h; only in the daemon holding the instance lock. Per runner it `try_lock`s a lock that run start, chat start, `finish` and runner removal also take; if busy it skips that runner this round. Run start never waits on the janitor, and the lock is never taken in the message loop.

| Job                  | Rule                                                                                                                                                                                                                                                    | Default     |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- |
| Transcript retention | Delete `history/runs/<run_id>.jsonl` older than `log_retention_days`, and files whose run is no longer in `runs_index.json`. Never the file of an active or recent (< 1 h) run. Tolerate a corrupt index: skip eviction-based deletion, keep age-based. | on, 14 days |
| Service log rotation | If `logs/runner.out.log` or `runner.err.log` exceeds 20 MB: copy the last 2 MB to `.1`, then truncate the live file in place (launchd writes in append mode). Skip if free disk is below 2 × the copy size.                                             | on, 20 MB   |
| Stale scratch        | Delete `pd-*` folders this instance recorded and that belong to no live run. Never other `pd-*` folders.                                                                                                                                                | on          |
| Reports only         | Size and location of pool-era folders, orphaned `runners/<id>/` data, and the latest escapee list, surfaced in `pidash doctor`.                                                                                                                         | on          |

**Path guard (`util/safe_remove.rs`, used by 4.2 and the janitor):** delete only paths under an allowed root that this daemon created (`data_dir/runners/<id>/history/runs/`, `data_dir/logs/`, recorded `pd-*` folders). Refuse a path that equals or contains a configured `working_dir`, the data dir itself, or anything resolving outside the allowed root. Open the root by descriptor and descend with `openat(O_NOFOLLOW | O_DIRECTORY)` at every level, unlinking relative to the parent descriptor, so a swapped ancestor or symlink cannot redirect the delete. Never delete a live session's scratch (checked under the runner lock).

### 4.6 Disk visibility

`pidash doctor` gains **Disk** (free space on the volumes of the data dir and of each working dir, warning below `[resources] warn_free_disk_gb`, default 2) and **Cleanup** (transcript size, scratch, pool-era folders, escapees). The daemon logs one warning per crossing of the threshold and triggers the janitor early. No dispatch change: admission is phase 2.

## 5. Configuration

```
[cleanup]                 # all optional; defaults shown
kill_escapees = false

[janitor]
enabled = true
interval_hours = 24
transcripts = true        # uses log_retention_days
rotate_service_logs_mb = 20

[resources]
warn_free_disk_gb = 2
```

Process ownership (4.1) and scratch folders (4.2) are always on: they only touch what the run itself created. Debug escape hatches (unsupported): `PIDASH_KEEP_RUN_TMP=1`, `PIDASH_NO_TREE_KILL=1`.

## 6. Compatibility

- No server or protocol change.
- `setsid` detaches agents from the terminal of a foreground `pidash __run`; Ctrl+C reaches only the daemon, which drains runs as today.
- launchd no longer kills agents on daemon crash by group; crash recovery (4.1) replaces it at next start. systemd's default `KillMode=control-group` is unaffected.
- Chat behaviour (4.2): processes a chat agent starts live until the chat runtime ends (Release before a run, idle timeout, Close). Documented.
- The janitor's first run logs one summary line (count, bytes).

## 7. Testing (written first)

- **pty test:** spawn through the helper from a pty-attached parent; the agent reaches `exec` (guards against the SIGTTIN hang).
- **Process tree:** a fake agent that starts `sleep 300 &` and a child that traps SIGTERM; both gone after finish, for normal finish, the `pump_events` error path, cancel and bridge drop. A child that ignores SIGTERM dies by SIGKILL. Leader exits first while children remain: the group is still cleaned. Generous time bounds.
- **Crash recovery:** kill the daemon without cleanup while the agent has a child, let the leader exit (leaderless group), restart: the group is cleaned. Also: a provisional record without pgid is resolved by tag; a record whose group members lack our tag and started before the recorded time is ignored; a reused pgid (leader alive with a different start time) and a leaderless group whose members started after the last-alive mark are not signalled; the start-time proof is also tested with environment reads off (the macOS path); another instance's record is ignored; a second daemon on the same data dir does not run recovery.
- **Ordering:** completion is published only after the tree is gone (assert ordering on finish, error path and cancel).
- **Proc records:** written atomically before spawn; unresolved records survive restart.
- **Escapees:** a helper that calls `setsid` (`perl -MPOSIX -e 'setsid; exec @ARGV'`, since macOS has no `setsid` binary) is reported, not killed by default; killed with `kill_escapees = true`; an identical process without the tag is untouched. Tests use a test-only instance id so they never touch the developer's real runs.
- **Scratch:** `TMPDIR` inside the agent equals the run folder even when an rc file sets `TMPDIR`; the path is short; the folder is gone after the run; path-guard refusals (symlink outside, working dir inside).
- **Janitor:** age and eviction retention, active-run protection, corrupt index, rotation with a live writer, low-disk skip.
- **Chat:** processes and scratch survive between turns of a live runtime and are cleaned on Release, idle timeout and Close.
- `cargo test --workspace --all-targets` and clippy `-D warnings` stay green on Linux CI; macOS verified by hand (no runner macOS CI job today).

## 8. Work breakdown (one PR each, in order)

1. **Process ownership:** `setsid`, `ProcessTree` with atomic pre-spawn records, `finish` replacing each bridge's shutdown on every stop path, completion published after cleanup, drop backstop, parallel daemon-shutdown finish, instance id + lock, `PIDASH_PROC_TAG` and the same-uid env reader (needed by crash recovery), crash recovery. Unix only; Windows keeps today's behaviour. Includes the design doc as `.ai_design/runner_cleanup/design.md`.
2. **Scratch folders:** short `pd-*` folder, `PIDASH_TMPDIR` re-export (Windows: direct `TMP`/`TEMP`), `safe_remove` with no-follow descent, chat runtime-end cleanup, deferred deletion while tagged processes remain.
3. **Janitor:** transcript retention, service log rotation, stale scratch, per-runner lock, report-only items.
4. **Escapee report** (reusing PR 1's env reader; rate-limited when runs end in bursts; opt-in kill with revalidation and allowlist) and **doctor Disk/Cleanup** checks.
5. **Windows Job Object** for `ProcessTree`.

Each PR is shippable alone and leaves behaviour no worse than before (PR 1 includes crash recovery so macOS does not regress).

## 9. Phase 2 (separate design, needs maintainer input)

- Disk/memory admission: an explicit resource lane in `recompute_status`, a non-blocking `statvfs` check in the message loop with hysteresis, a visible reason in the TUI and on the server, and a verified server re-dispatch behaviour (today a busy NACK flips the runner to BUSY with no in-flight run).
- Memory sampling and per-run limits (Linux cgroups via `systemd-run --user --scope`, Windows Job Object limits, a macOS watchdog), resource hints in the run env, forgotten-server detection.
- Opt-in deletion of pool-era folders after checking `git status` is clean and nothing is unpushed; opt-in build-cache pruning.
- Open questions: limits on by default or opt-in; ending runs on `memory_limit`; dedicated `disk_low`/`memory_limit` reasons and a "degraded" runner status (protocol change).
