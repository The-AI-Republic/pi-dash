# Runner TUI — Design

The MVP runner ships with a built-in TUI so users can configure the runner, watch it work, and answer approval requests from a terminal. This complements (not replaces) the Pi Dash web UI.

## Why a TUI for v1

| Option               | Effort | UX for laptop users                                     | Verdict           |
| -------------------- | ------ | ------------------------------------------------------- | ----------------- |
| CLI only (GHA shape) | Small  | Poor — invisible daemon, users don't know if it's alive | Wrong for v1      |
| TUI                  | Medium | Good — standard dev-tool shape (k9s, lazygit, gh-dash)  | **Right for MVP** |
| Native tray icon     | Large  | Best — glanceable                                       | Defer to v2       |
| Full desktop app     | Huge   | Overkill                                                | Skip              |

A TUI also gives the developer-user immediate feedback during first-time setup ("is it connected? does it see Codex?"), which prevents the single biggest support issue: "I installed it and nothing happened."

## Architecture: daemon + TUI client over local IPC

The TUI is a **client**, not the runner itself. The runner daemon keeps running in the background (as a service) regardless of whether the TUI is open. The TUI attaches, reads state, sends commands, and detaches.

```
┌────────────────────── laptop ──────────────────────┐
│                                                     │
│   pidash  (service, always-on)        │
│        │                                            │
│        │  local Unix socket                         │
│        │  (Windows: named pipe)                     │
│        │                                            │
│   pidash tui  (user launches adhoc)   │
│        │                                            │
│        │  ─────► outbound WS to cloud               │
│        │  ─────► spawn codex app-server             │
│                                                     │
└─────────────────────────────────────────────────────┘
```

Why this shape:

1. **TUI launches and exits cheaply.** Open it with `runner tui`, close with `q`, runner keeps working. No restart involved.
2. **Multiple concurrent TUIs are safe.** User can tail status from two terminal tabs.
3. **Same local-IPC surface will power the future tray icon.** The tray is just another client of the same socket.
4. **Config changes apply live.** TUI POSTs to the daemon via the socket; daemon writes to disk and hot-applies. No process restart for most edits.
5. **Decouples rendering from the work loop.** The daemon's core logic never blocks on TUI rendering.

### Local IPC protocol

Use JSON-RPC 2.0 over the Unix socket / named pipe. Same protocol style the runner uses toward Codex and toward the cloud — consistent across the codebase.

Minimum methods (daemon-side):

- `status.get` → current state snapshot (connection, current run, Codex health, config summary)
- `status.subscribe` → stream of state deltas for the live view
- `config.get` / `config.update` → read/write config (labels, approval policy, etc.)
- `runs.list` → recent runs with pagination
- `runs.get(id)` → detail + event stream
- `approvals.list` → pending approvals
- `approvals.decide(id, decision)` → answer an approval request
- `runner.reregister` / `runner.disconnect` → lifecycle control
- `doctor.run` → verify Codex installed + logged in, git config, network reachable

Socket path:

- Linux/macOS: `$XDG_RUNTIME_DIR/pidash.sock` (or `~/.local/share/pidash/sock`)
- Windows: `\\.\pipe\pidash`

Permissions: 0600 — only the owning user can connect.

## Library choice

**Go path (recommended stack):**

- [Bubble Tea](https://github.com/charmbracelet/bubbletea) — Elm-architecture TUI framework from Charm.
- [Bubbles](https://github.com/charmbracelet/bubbles) — ready-made components (list, viewport, text input, table, spinner).
- [Lip Gloss](https://github.com/charmbracelet/lipgloss) — declarative styling/layout.

Rationale: the Charm ecosystem is the de-facto standard for new Go TUIs in 2026. `k9s`, `gh dash`, `glow`, `soft-serve`, and others all use it. Low risk, high ergonomics, large community.

**Rust path (if runner is in Rust):**

- [Ratatui](https://github.com/ratatui/ratatui) — the standard modern Rust TUI (formerly `tui-rs`).
- [Crossterm](https://github.com/crossterm-rs/crossterm) — terminal backend.

Equally mature. Slightly more verbose than Bubble Tea but equally capable.

## Views for MVP (v1 scope)

Four primary views. Switch with number keys or tab. Vim-style `j/k/h/l` + arrow keys for navigation. `?` for help overlay everywhere.

### 1. Status (default landing view)

```
╭─ Pi Dash Runner ─────────────────────────────────────╮
│ ● Connected        my-laptop                               │
│ Workspace: acme    Labels: codex, macos, arm64             │
│ Uptime 4h 23m      Last heartbeat 2s ago                   │
├────────────────────────────────────────────────────────────┤
│ Preconditions                                              │
│   ✓ Codex binary     /usr/local/bin/codex  (v0.45.0)       │
│   ✓ Codex auth       user@example.com (ChatGPT Plus)      │
│   ✓ Git              2.43.0                                 │
│   ✓ Network          cloud reachable                       │
├────────────────────────────────────────────────────────────┤
│ Current run                                                 │
│   PROJ-123 — Refactor auth module                          │
│   Status: running   Thread: th_01HX…                       │
│   Started 12:34:56  Events 127                             │
│                                                             │
│   Recent events                                             │
│     12:35:03  tool_call   shell: git status                │
│     12:35:09  item/delta  "Reading src/auth/…"             │
│     12:35:12  tool_call   write: src/auth/session.py       │
├────────────────────────────────────────────────────────────┤
│ [1]Status [2]Runs [3]Config [4]Approvals  [?]Help  [q]Quit │
╰────────────────────────────────────────────────────────────╯
```

Glanceable summary: Am I connected? Is Codex healthy? What am I doing right now?

### 2. Runs

A scrollable table of recent `AgentRun`s (pulled from the daemon's cache; daemon keeps last N=100).

```
╭─ Runs ─────────────────────────────────────────────────────╮
│ ID        Work Item               Status      Started       │
├────────────────────────────────────────────────────────────┤
│ r_0xA3   PROJ-123 Refactor auth   ▶ running   12:34         │
│ r_0xA2   PROJ-119 Fix flaky test  ✓ done      11:02         │
│ r_0xA1   PROJ-118 Docs typo       ✓ done      10:55         │
│ r_0xA0   PROJ-115 Upgrade deps    ✗ failed    10:12         │
│ …                                                          │
╰────────────────────────────────────────────────────────────╯

↵ Open detail   f Filter   / Search   [esc] Back
```

Detail view (`↵` on a row): shows the event stream + final diff + any approvals.

### 3. Config

Form-style editor. Edits are POSTed to the daemon and applied live.

```
╭─ Configuration ────────────────────────────────────────────╮
│ Identity                                                   │
│   Runner name     my-laptop                                │
│   Workspace       acme                                     │
│   Cloud URL       http://localhost           │
│                                                             │
│ Capabilities (labels)                           [e] edit   │
│   codex, macos, arm64                                      │
│                                                             │
│ Codex                                                       │
│   Binary path     /usr/local/bin/codex           ✓ found   │
│   Model default   gpt-5-codex                              │
│                                                             │
│ Approval policy                                             │
│   [x] Auto-approve read-only shell                         │
│   [ ] Auto-approve writes inside workspace                 │
│   [ ] Auto-approve network calls                           │
│                                                             │
│ Logging                                                     │
│   Level           info  ▾                                  │
│   Retention       7 days                                   │
│                                                             │
│ [s] Save   [r] Re-register   [d] Disconnect   [esc] Back   │
╰────────────────────────────────────────────────────────────╯
```

Behaviors:

- `s` persists and hot-applies what can be, flags what needs a restart.
- `r` (re-register) prompts for a new registration token, tears down the current identity, and reconnects.
- `d` (disconnect) de-registers from cloud and stops the daemon.

### 4. Approvals

The most interactive view. Shows pending approval requests from Codex. Answering one pushes the decision back through the daemon → WS → cloud → runner → `codex app-server`.

```
╭─ Pending approvals (1) ────────────────────────────────────╮
│ PROJ-123 — Refactor auth module                            │
│                                                             │
│ Codex wants to run shell command:                          │
│                                                             │
│   $ rm -rf /Users/rich/workspace/acme/.cache               │
│                                                             │
│ Working dir: /Users/rich/workspace/acme                    │
│ Reason: "Clearing stale build cache before next step"      │
│                                                             │
│ Exposure: filesystem write (destructive)                   │
├────────────────────────────────────────────────────────────┤
│ [a] Approve once                                           │
│ [A] Approve for this session                               │
│ [d] Decline                                                │
│ [c] Cancel run                                             │
│ [esc] Back                                                 │
╰────────────────────────────────────────────────────────────╯
```

Notes:

- Daemon can also auto-answer based on the configured approval policy; TUI shows only what needs a human.
- When a new approval arrives and the TUI is open, focus jumps to this view and the terminal bell rings (unless user disabled).
- Same `ApprovalRequest` object is also exposed to the Pi Dash web UI; whichever answers first wins. Daemon records the decision source for audit.

## Key bindings summary

| Key            | Action                                |
| -------------- | ------------------------------------- |
| `1`–`4`        | Jump to view                          |
| `j/k` or `↑/↓` | Move selection                        |
| `↵`            | Open / activate                       |
| `esc`          | Back                                  |
| `/`            | Search in current view                |
| `?`            | Help overlay                          |
| `r`            | Refresh                               |
| `q`            | Quit TUI (daemon keeps running)       |
| `Q`            | Quit and stop daemon (confirm prompt) |

`Q` requires a confirmation step ("Stop the runner daemon? [y/N]") because stopping a live run is destructive.

## First-run onboarding flow

When the user runs `pidash tui` and no config exists on disk, the TUI walks them through setup instead of showing the dashboard:

```
Step 1 of 4: Paste registration code
  Get a one-time code from Pi Dash → Settings → Runners → New runner.
  Code:  __________

Step 2 of 4: Verify Codex
  Checking Codex installation...
    ✓ codex v0.45.0 found at /usr/local/bin/codex
    ✓ logged in as user@example.com

Step 3 of 4: Choose labels
  Default labels:  self-hosted, macos, arm64
  Add custom:      codex, acme-repo
  [x] acme-repo    [ ] personal-repo

Step 4 of 4: Install as service
  [x] Start on login (launchd)

  [Finish]
```

Steps that detect problems (Codex missing, auth expired) become interactive remedies rather than errors.

## Accessibility and terminal compatibility

- Target: modern emulators that support truecolor + mouse — iTerm2, Alacritty, Wezterm, Windows Terminal, GNOME Terminal, Konsole, Kitty.
- Gracefully degrade to 256-color mode. Detect with `$COLORTERM`.
- Respect `$NO_COLOR`. Provide a `--no-color` flag.
- Every action reachable by keyboard; mouse is nice-to-have.
- Do not rely on Unicode-specific box-drawing for critical affordances; glyphs degrade on plain ASCII terminals but content is still readable.
- Respect `$TERM` variations. Refuse to start and print a clear message on `dumb`/`xterm-mono`.

## Out of scope for v1

Defer these to v2+:

- Multi-pane layout configurable by user.
- Theme support beyond a default + a light variant.
- Remote TUI (attaching to another machine's runner over the network). GHA doesn't do this either; unnecessary complication.
- Rich text-editor-style config inputs. Keep it form-based.
- Full run replay (ability to scroll back through an entire historical thread's events). Show recent-N only; point users to the web UI for full history.

## Testing strategy

- **Golden-frame tests.** Use `teatest` (Bubble Tea's testing harness) to drive the TUI state and assert rendered output against golden files.
- **IPC contract tests.** Spin up the daemon with an in-memory cloud + in-memory Codex mock; drive it through the JSON-RPC socket and assert state transitions.
- **Real-terminal smoke test.** A single CI job that boots the TUI in a `tmux` session and checks it renders the Status view without crashing on each supported OS.
- **Manual QA matrix per release.** macOS Terminal, iTerm2, Linux GNOME Terminal, Windows Terminal — render the four main views, resize the window, trigger an approval.

## Phasing within the TUI itself

**v1.0 (MVP):**

- Status view (read-only) with live event stream for the current run.
- Config view with read + write.
- Approvals view with approve/decline.
- First-run onboarding wizard.

**v1.1:**

- Runs view with history table + detail.
- Doctor subcommand surfacing health checks.
- Search / filter in Runs view.

**v1.2:**

- Per-run diff preview (the unified diff Codex is about to propose).
- Log viewer with follow mode.
- Help overlay with searchable keybinding list.

**v2 and beyond (may unlock the tray icon):**

- Expose the daemon's IPC socket to a native tray client (same contract, different presenter).
- Optional themes.
- Plugin hooks for custom status panels.
