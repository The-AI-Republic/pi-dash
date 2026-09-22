# Upgrade note: the worktree pool is gone (one work dir per runner)

Older runners could share a single repo checkout across several runners with a
**worktree pool**: a `[[workdir]]` table named a canonical git clone, runners
referenced it with `workdir = "<name>"`, and each run executed in a git
worktree leased from that pool.

Pi Dash removed the pool. A runner now resolves to **exactly one working
directory** — the `working_dir` under its `[runner.workspace]` table — and both
chat and issue runs execute there directly. The `[[workdir]]` tables and the
per-runner `workdir = "..."` key no longer exist.

On the first load after upgrading, the runner reconciles an existing
`config.toml` against the new shape. What happens depends on how your pool was
set up.

## Case 1 — one runner referencing one `[[workdir]]` (the common case)

Nothing to do. The runner is pointed directly at the pool's canonical clone
(`[[workdir]].path`) — the same repository its worktrees were sourced from — and
starts normally. You'll see a one-line warning in the daemon log recording the
change, for example:

```
runner "codex": now runs directly in "/home/me/repo" (the canonical clone of
removed pool "repo"); its previous working_dir "…/workspace" was a pool
placeholder and is no longer used.
```

The only behavioural difference is that runs execute in the clone itself
instead of in a throwaway worktree. This is reported rather than silent so you
know where the agent now works.

## Case 2 — several runners sharing one `[[workdir]]`

Only one runner can keep the shared directory now. The upgrade picks the
**first runner in config order** as the winner: it keeps the canonical clone
(`[[workdir]].path`), exactly like Case 1. Every other runner that referenced
the same pool is moved to its **own new working directory** —
`<data_dir>/workspaces/<project-slug>_<runner-slug>_<id>`, the same location a
fresh `pidash runner add` (with no `--working-dir`) would have chosen. The
daemon starts, and each move is recorded in the log, for example:

```
runner "codex": keeps working_dir "/home/me/repo" — formerly the canonical
clone of removed pool "repo". Runs now execute here directly instead of in a
leased worktree.
runner "claude": shared removed pool "repo" with another runner, which keeps the
canonical clone "/home/me/repo". This runner now has its own working_dir
"…/workspaces/test_claude_1a2b3c4d" (created empty; the repo is cloned into it on
first run). Its previous working_dir "/home/me/repo" is no longer used.
```

Nothing moves silently — every displaced runner is named in the log with both
its old and new directory.

### Why a displaced runner needs a clone, not just an empty folder

A runner's working directory isn't a bare scratch folder — it has to contain a
**checkout of the project repository**, because that's where the agent reads and
edits code, commits, and pushes. Under the old pool, the sharing runners didn't
each own a copy: they shared one canonical clone and got cheap git worktrees off
it. Splitting them to one-dir-per-runner means each displaced runner needs its
own copy of the repo.

That copy is created **lazily, not at upgrade time**. The new directory starts
empty; on the runner's first run the cloud hands it the repository URL, and
`workspace::resolve` clones into the empty directory then (an empty dir + a repo
URL → `git clone`). So the upgrade itself does no network I/O and can't be slow —
you just pay a one-time clone the first time each displaced runner actually runs.
If you'd rather not wait for that first-run clone, you can pre-populate the new
directory with your own clone of the repo; the runner will detect the existing
`.git` and use it as-is.

## Case 3 — legacy runners with `working_dir` and no `[[workdir]]`

Nothing to do and nothing changes. These runners already had one directory each.
The one-directory-per-runner validation (no two runners may share or nest
`working_dir`) now applies to them too, but a config that was valid before stays
valid.

## Leftover directories are never deleted

The pool's worktrees and any per-runner chat worktree may contain uncommitted
agent work, so the upgrade **never removes them** — they are kept in place for
backward compatibility. The new build simply stops using them; it reports their
locations so you can recover anything you need and clean up on your own schedule:

- pool worktrees: `<data_dir>/worktrees/<workdir-name>/` (or the `worktrees_dir`
  override, if the `[[workdir]]` set one);
- chat worktrees: `<data_dir>/runners/<runner-id>/chat-worktree/`.

`<data_dir>` is the runner's XDG data directory (e.g.
`~/.local/share/pidash/` on Linux).

## Cleaning up the old keys

The removed `[[workdir]]` tables and `workdir = ...` lines are ignored once
you're on the new build. You can delete them by hand; they are also rewritten
out of `config.toml` automatically the next time the config is mutated
(`pidash runner add` / `pidash runner remove`).
