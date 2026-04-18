# Runner — End-User Setup Guide

This guide walks you through connecting your dev machine to Apple Pi Dash so the cloud can assign coding tasks to Codex running locally.

**Who this is for:** developers who want their Apple Pi Dash issues to become live Codex runs on their own hardware.

**What you need before you start:**

- Apple Pi Dash account with access to your workspace
- Codex installed and logged in (`codex --version` works, `codex account status` shows your account)
- Git installed with working credentials for the repos you'll work on
- macOS (arm64/x64) or Linux (x64). Windows is not yet supported.

## 1. Install the runner

Pick one:

```bash
# Homebrew (macOS + Linux)
brew install apple-pi-dash/tap/runner

# Shell installer
curl -fsSL https://cloud.apple-pi-dash.so/install.sh | sh
```

Both drop `apple-pi-dash-runner` into `$HOME/.local/bin`. Make sure that directory is in your `PATH`.

## 2. Get a registration code

1. Sign in to `https://cloud.apple-pi-dash.so` (or your self-hosted URL).
2. Open your workspace → **Runners** tab.
3. Click **Mint registration code**.
4. Copy the code. It's single-use and expires in 1 hour.

You can register up to 5 runners per account per workspace.

## 3. Configure the runner

### Option A — interactive TUI (recommended for first-time setup)

```bash
apple-pi-dash-runner tui
```

The TUI detects that no config exists and walks you through 4 steps:

1. **Cloud URL** — prefilled with the default; change if you're self-hosting.
2. **Registration code + runner name** — paste the code you copied; the name defaults to your hostname.
3. **Preflight** — the wizard runs `codex --version`, `codex account status`, and `git --version`. Any red ✗ means fix that before continuing (usually `codex login` or installing git).
4. **Install as service** — tick the box to start the daemon at login.

### Option B — command-line (scripts / CI)

```bash
apple-pi-dash-runner configure \
  --url https://cloud.apple-pi-dash.so \
  --token <PASTE_REGISTRATION_CODE> \
  --name my-laptop

apple-pi-dash-runner service install
apple-pi-dash-runner service start
```

The daemon now runs as a `launchd` agent (macOS) or `systemd --user` unit (Linux) and will auto-start on login.

## 4. Verify it's connected

```bash
apple-pi-dash-runner status
```

You should see `connected` and your cloud URL. Equivalently, open the web UI → **Runners** tab; the new runner appears with a green **online** badge within a few seconds.

## 5. Attach the TUI dashboard

```bash
apple-pi-dash-runner tui
```

Four tabs:

- **Status** — connection state, current run, Codex/git health
- **Runs** — recent runs on this machine (last 100)
- **Config** — live config view (edit with `e` on supported fields)
- **Approvals** — pending approval requests

Press `?` anywhere for help. `q` closes the TUI (daemon keeps running). `Q` stops the daemon entirely.

## 6. Respond to approval requests

When Codex tries to do something risky (write outside the workspace, run `git push`, network access, etc.) it pauses and asks for approval. You'll see it in **two** places:

- The runner's **Approvals** TUI tab — the terminal bell rings and the view auto-focuses.
- The web UI's **Runners → Approvals** tab — shows every pending approval across all your runners.

Pick either surface; whichever answers first wins. The other surface will clear automatically.

Actions:

- **Accept once** — Codex proceeds with this one action.
- **Accept for session** — remembered for the rest of the current thread, so similar requests auto-pass.
- **Decline** — Codex sees denial and usually continues with an alternative.
- **Cancel run** — kill the whole run.

Pending approvals expire after 10 minutes; the server then cancels the run automatically.

## 7. Day-to-day

- **Your working directory** lives at `$TMPDIR/.apple_pi` by default. Change with `apple-pi-dash-runner configure --working-dir /path/to/repo` or the Config tab. Runs reuse this directory; Codex is expected to reset/stash/checkout at the start of each task.
- **Logs** are in `~/.local/share/apple-pi-dash-runner/logs/`.
- **Per-run transcripts** are stored as JSONL in `~/.local/share/apple-pi-dash-runner/history/runs/`.

## 8. Upgrading

```bash
brew upgrade apple-pi-dash-runner
apple-pi-dash-runner service stop
apple-pi-dash-runner service start
```

A protocol version bump will be announced in the release notes. The daemon logs a warning if the server expects a newer version; in that case, upgrade.

## 9. Rotating your runner credential

If you suspect your runner secret has leaked:

```bash
apple-pi-dash-runner rotate
apple-pi-dash-runner service stop
apple-pi-dash-runner service start
```

The old secret is invalidated immediately. No downtime on the runner record in the cloud — just a forced WS reconnect with the new secret.

## 10. Removing

```bash
apple-pi-dash-runner service stop
apple-pi-dash-runner service uninstall
apple-pi-dash-runner remove
```

This deregisters the runner in the cloud, deletes local credentials and config, and stops the agent. Your transcripts under `~/.local/share/apple-pi-dash-runner/history/` are NOT deleted automatically — remove them manually if you want.

## Troubleshooting

**`status` shows disconnected** → daemon isn't running. Check `apple-pi-dash-runner service status`. If the unit is loaded but not active, `service start` it. If systemd says "failed", check `journalctl --user -u apple-pi-dash-runner`.

**`doctor` says `codex-auth: unable to confirm`** → run `codex login`. Re-run `doctor` to confirm.

**`workspace_setup` failure on a new run** → check that your working directory either contains a git repo or is empty. The daemon refuses to operate on a non-empty non-repo directory to avoid clobbering anything.

**Approvals TUI beeps but no popup** → the wizard only auto-focuses when the TUI is on a tab other than Approvals. Press `4` to go to it.
