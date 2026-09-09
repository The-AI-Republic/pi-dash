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

This cannot migrate automatically: only one runner can keep the shared
directory, and silently moving the others would run their agents somewhere you
didn't choose. The daemon **refuses to start** and prints the exact edit, for
example:

```
configuration error: the worktree pool was removed (PDASHOSS01-134), but
config.toml still shares one work dir across several runners: runners "codex",
"claude" shared workdir "repo" (canonical clone "/home/me/repo"). Each runner
now needs its own working directory. Keep one runner's [runner.workspace]
working_dir at the canonical clone shown above, and point every other sharing
runner at a distinct directory — an empty path is fine, the daemon clones the
repo into it on first run. Then delete the [[workdir]] tables and the
`workdir = ...` lines.
```

Edit `config.toml` accordingly: keep one runner on the canonical clone, and give
each other runner its own `working_dir`. A directory that doesn't exist yet (or
is empty) is fine — the daemon clones the repository into it on first run, so
you don't have to pre-populate anything.

## Case 3 — legacy runners with `working_dir` and no `[[workdir]]`

Nothing to do and nothing changes. These runners already had one directory each.
The one-directory-per-runner validation (no two runners may share or nest
`working_dir`) now applies to them too, but a config that was valid before stays
valid.

## Leftover directories are never deleted

The pool's worktrees and any per-runner chat worktree may contain uncommitted
agent work, so the upgrade **never removes them**. It reports their locations so
you can recover anything you need and clean up on your own schedule:

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
