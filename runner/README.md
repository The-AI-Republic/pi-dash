# Pi Dash Runner

Local daemon + TUI (`pidash` binary) that connects a developer machine to the Pi Dash cloud and drives `codex app-server` for assigned tasks.

## Install

Prebuilt binaries for macOS (arm64, x86_64), Linux (arm64, x86_64), and Windows (x86_64) are published to GitHub Releases. The one-liners below download the installer, verify checksums, drop `pidash` into the standard install path, and immediately start the device-code login so the host is registered with your Pi Dash cloud before you leave the terminal.

**macOS and Linux:**

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.ps1 | iex
```

The installer will prompt for your Pi Dash cloud URL, walk you through device-code approval in the browser, and offer to register this host as a runner — for the typical dev-laptop case, that's the entire setup.

### Installing a specific version (pinning / prereleases)

Both the wrapper (`install.sh` / `install.ps1`) and the underlying cargo-dist installer (`pidash-installer.sh` / `pidash-installer.ps1`) select a release purely by URL path — swap `latest` for the tag you want:

```bash
# macOS / Linux — wrapper (with auto-auth)
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/download/pidash-v0.1.0/install.sh | sh

# macOS / Linux — bare installer (no auto-auth)
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/download/pidash-v0.1.0/pidash-installer.sh | sh
```

```powershell
# Windows — wrapper
irm https://github.com/The-AI-Republic/pi-dash/releases/download/pidash-v0.1.0/install.ps1 | iex

# Windows — bare installer
irm https://github.com/The-AI-Republic/pi-dash/releases/download/pidash-v0.1.0/pidash-installer.ps1 | iex
```

**Prereleases** (tags with a SemVer suffix like `-rc.1`, `-alpha.1`, `-beta.1`) are deliberately excluded from `/releases/latest/`, so the public one-liners above keep serving the last stable release. To install a prerelease for testing, use the tag-pinned URL above with the rc tag, e.g.:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/download/pidash-v0.1.5/install.sh | sh
```

**Prerequisite:** the runner shells out to [Codex](https://github.com/openai/codex) — install it and make sure `codex --version` works before running `pidash configure`. `pidash doctor` checks this.

### Alternative install paths

If you'd rather install the binary without the auto-auth (e.g. for a base image, MSI-based deploy, or any CI context where auth happens later), use the underlying cargo-dist installers directly:

```bash
# macOS / Linux
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.sh | sh
```

```powershell
# Windows PowerShell
irm https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.ps1 | iex
```

Or on Windows, download and run the MSI installer:

<https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-x86_64-pc-windows-msvc.msi>

The MSI is a standard double-click Windows Installer — it drops `pidash.exe` and exits. It does **not** automatically launch the auth flow (MSI typically runs elevated as the Windows Installer service, and many MSI invocations are silent for IT/Group Policy deploys, so spawning a terminal from it would be wrong). To finish setup after the MSI closes:

```powershell
# Open PowerShell and run:
pidash
```

With no config yet, bare `pidash` drops into `auth login` and walks you through the same device-code flow as the `install.ps1` one-liner. (A discoverable Start Menu shortcut for this step is tracked as a follow-up.)

Windows release assets also include a `pidash-x86_64-pc-windows-msvc.zip` archive with `pidash.exe` for advanced/manual installs.

Then run the setup steps manually:

```bash
# 1. Log in as your user. Opens a browser to approve a short code shown in
#    the terminal — same idea as `gh auth login` or `stripe login`. Stores
#    a CLI token at ~/.config/pidash/config.toml.
pidash auth login --url https://pidash.example.com

# 2. Register this host as a runner. Uses the token from step 1 to mint
#    runner credentials cloud-side; no enrollment-token paste needed. On
#    the first runner, installs the systemd user unit, launchd agent, or
#    Windows per-user scheduled task and starts the daemon.
pidash runner add --project WEB

# 3. Optional: open the interactive UI.
pidash tui
```

`pidash auth login` prompts to add a runner inline when no runner exists yet on the host — for the dev-laptop case, that single command is enough. Bare `pidash` with no subcommand also drops into the login flow when no config exists, so if you installed via the MSI or skipped auto-auth, you can re-trigger setup just by typing `pidash`.

Useful follow-ups:

```bash
pidash auth status               # who am I logged in as; runner list
pidash auth logout               # revoke the CLI token server-side
pidash runner add --project X    # add another runner
pidash runner list / remove      # manage runners
```

### Alternative: legacy enrollment-token flow

For headless / scripted setup where you can't run the browser flow, the original `pidash connect` enrollment-token paste still works:

```bash
# Generate a one-time enrollment token from the Pi Dash web UI
# (Runners admin → "Add connection"), then on the target host:
pidash connect --url https://pidash.example.com --token <ONE_TIME_TOKEN>
```

This path is kept as a fallback; the device-code flow above is the recommended path for everyone with browser access.

## Auto-update

`pidash` keeps itself current. When the cloud announces a newer `latest_runner_version` in the welcome frame, the running daemon swaps the on-disk `pidash` binary in place. The currently-running process is **never disturbed** — it keeps its loaded copy until the next natural restart (`pidash restart`, host reboot, or a service-manager respawn after a crash). This gives you the Claude-Code-style "always current" experience without ever killing in-flight work.

The toggle lives in the General tab's **Daemon settings** card (`pidash tui` → `auto_update`). Default is **on**; press Enter to flip, then `[w]` to save. With auto-update off, the runner instead surfaces a yellow `⚠ Update v0.1.x available` advisory in the Connection card and on `pidash status`, and you apply updates manually:

```bash
pidash update              # swap binary; tells you to run pidash restart
pidash update --check      # report whether an update is available
pidash update --restart    # swap and restart the daemon in one shot
```

`pidash update` only works for binaries installed via cargo-dist installers such as `pidash-installer.sh`, `pidash-installer.ps1`, or the Windows MSI (it reads the cargo-dist install receipt). Source builds, zip-only installs, and `cargo install`'d binaries don't have a receipt and get a clear "reinstall via the installer if you want self-update" error.

### What the advisory states mean

| State                                                     | TUI / `pidash status`                                  |
| --------------------------------------------------------- | ------------------------------------------------------ |
| Running version ≥ `latest_announced` and ≥ `min_required` | nothing shown                                          |
| Newer `latest_announced`, swap already on disk            | yellow `⚠ Restart to apply v0.1.x`                     |
| Newer `latest_announced`, auto-update on, swap pending    | yellow `⚠ Update v0.1.x pending swap`                  |
| Newer `latest_announced`, auto-update off                 | yellow `⚠ Update v0.1.x available — run pidash update` |
| Running version below `min_required` (cloud-set floor)    | red `⛔ Update required: cloud floor v0.1.x`           |

`min_required` is advisory in the current implementation — the daemon does not refuse new tasks below the floor. The red banner is the user-facing signal that they should act before the cloud bumps the wire-protocol floor and disconnects them.

### Announcing a release from the cloud

The Pi Dash backend reads two optional environment variables and folds them into every session-create welcome response:

| Env var                 | Effect                                                                                    |
| ----------------------- | ----------------------------------------------------------------------------------------- |
| `LATEST_RUNNER_VERSION` | Drives the yellow "update available" advisory and triggers auto-swap on opted-in runners. |
| `MIN_RUNNER_VERSION`    | Drives the red "update required" advisory.                                                |

Set both after cutting a runner release (`RELEASING.md` walks through tagging). Leave them unset to skip the announcement.

## Design docs

See `.ai_design/implement_runner/`:

- `runner-design.md` — architecture + committed decisions
- `github-runner-architecture.md` — reference model (GHA self-hosted runner)
- `tui-design.md` — TUI shape + views

## Layout

```
runner/
├── Cargo.toml                # binary crate, edition 2024, MSRV 1.93
├── src/
│   ├── main.rs               # tokio entrypoint
│   ├── lib.rs                # module root
│   ├── cli/                  # clap subcommands: configure / install / uninstall / start / stop / restart / status / tui / doctor / remove / rotate / issue / comment / state / workspace / __run (hidden)
│   ├── daemon/               # supervisor + state machine
│   ├── cloud/                # WS client, message schemas, registration HTTP
│   ├── codex/                # app-server subprocess + JSON-RPC bridge
│   ├── workspace/            # working_dir resolution + `git clone` on first task
│   ├── approval/             # policy engine + first-writer-wins router
│   ├── ipc/                  # local IPC between daemon and TUI/CLI
│   ├── history/              # JSONL per-run transcripts + recent-runs index
│   ├── service/              # systemd / launchd / Windows scheduled-task setup
│   ├── tui/                  # Ratatui app + views (Status / Runs / Config / Approvals)
│   ├── config/               # TOML config + credential files (0600)
│   └── util/                 # paths, logging, backoff, signal handling
└── tests/                    # integration tests
```

## Development commands

```bash
cargo build                                  # debug build
cargo test                                   # unit + integration tests
cargo check                                  # quick type-check
cargo clippy -- -D warnings                  # lint
```

From a debug build, substitute `./target/debug/pidash` for `pidash` in any of the commands above.

## Runtime paths

- Config: platform config directory for `pidash`
- Data / logs: platform data directory for `pidash`
- Runtime dir: platform runtime directory for local IPC and the PID file

On Unix-like systems, secrets on disk are written with `0600` and the IPC socket is also `0600`. On Windows, files are stored under the user's profile directories and daemon IPC uses a local named pipe.

## Protocol

Wire version is `4` — bumped on incompatible shape changes. See `src/cloud/protocol.rs` for the exhaustive schema (including the v3→v4 move from WebSocket to per-runner HTTPS long-poll). The runner authenticates to the cloud with a per-runner access token issued from a refresh-token pair; the cloud echoes an accepted `protocol_version` and may include optional `latest_runner_version` / `min_runner_version` advisories in the `welcome` payload (consumed by the auto-update path).

## Test strategy

- **Unit:** `cargo test` — deterministic table-driven tests for protocol serde, approval policy, reconnect backoff, workspace resolve, config roundtrip.
- **Integration:** `tests/protocol_roundtrip.rs` — every client/server variant round-trips; router state machine invariants.
- **Manual QA** (per release): macOS arm64/x64 + Linux x64 + Windows x64 -> first-run `configure` -> `install` -> `start` -> TUI shows connected -> synthetic run via `/api/runners/runs/` -> approval prompt -> decision.

## Release

Managed by `cargo-dist` (see `dist-workspace.toml` + `.github/workflows/release.yml`). Pushing a SemVer tag (e.g. `v0.1.0`) triggers the workflow, which builds binaries for macOS arm64/x64, Linux arm64/x64, and Windows x64, generates shell, PowerShell, and MSI installers, and publishes everything to a GitHub Release.

To cut a release:

```bash
# bump version in runner/Cargo.toml, commit, then:
git tag v0.1.0
git push origin v0.1.0
```
