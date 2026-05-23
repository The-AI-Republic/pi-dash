# Pi Dash Runner

Local daemon + TUI (`pidash` binary) that connects a developer machine to the Pi Dash cloud and drives `codex app-server` for assigned tasks.

## Install

Prebuilt binaries for macOS (arm64) and Linux (arm64, x86_64) are published to GitHub Releases. The one-liner below downloads the installer, verifies checksums, drops `pidash` into `$HOME/.local/bin`, and immediately starts the device-code login so the host is registered with your Pi Dash cloud before you leave the terminal:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.sh | sh
```

The installer will prompt for your Pi Dash cloud URL, walk you through device-code approval in the browser, and offer to register this host as a runner — for the typical dev-laptop case, that's the entire setup.

Pin to a specific version by swapping `latest` for a tag, e.g. `.../releases/download/pidash-v0.1.0/install.sh`.

**Prerequisite:** the runner shells out to [Codex](https://github.com/openai/codex) — install it and make sure `codex --version` works before running `pidash configure`. `pidash doctor` checks this.

If you'd rather install the binary without the auto-auth (e.g. for a base image where auth happens later), use the underlying cargo-dist installer directly:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.sh | sh
```

Then run the setup steps manually:

```bash
# 1. Log in as your user. Opens a browser to approve a short code shown in
#    the terminal — same idea as `gh auth login` or `stripe login`. Stores
#    a CLI token at ~/.config/pidash/config.toml.
pidash auth login --url https://pidash.example.com

# 2. Register this host as a runner. Uses the token from step 1 to mint
#    runner credentials cloud-side; no enrollment-token paste needed. On
#    the first runner, installs the systemd user unit / launchd agent and
#    starts the daemon.
pidash runner add --project WEB

# 3. Optional: open the interactive UI.
pidash tui
```

`pidash auth login` prompts to add a runner inline when no runner exists yet on the host — for the dev-laptop case, that single command is enough. Bare `pidash` with no subcommand also drops into the login flow when no config exists, so if you skipped auto-auth at install time you can re-trigger it just by typing `pidash`.

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

`pidash update` only works for binaries installed via `pidash-installer.sh` (it reads the cargo-dist install receipt). Source builds and `cargo install`'d binaries don't have a receipt and get a clear "reinstall via the installer if you want self-update" error.

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
│   ├── ipc/                  # Unix-socket IPC between daemon and TUI/CLI
│   ├── history/              # JSONL per-run transcripts + recent-runs index
│   ├── service/              # systemd / launchd unit generators
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

## Runtime paths (XDG)

- Config: `~/.config/pidash/`
- Data / logs: `~/.local/share/pidash/`
- Runtime dir: `$XDG_RUNTIME_DIR/pidash/` (Unix socket, PID file)

All secrets on disk are written with `0600`. The Unix IPC socket is also `0600`.

## Protocol

Wire version is `4` — bumped on incompatible shape changes. See `src/cloud/protocol.rs` for the exhaustive schema (including the v3→v4 move from WebSocket to per-runner HTTPS long-poll). The runner authenticates to the cloud with a per-runner access token issued from a refresh-token pair; the cloud echoes an accepted `protocol_version` and may include optional `latest_runner_version` / `min_runner_version` advisories in the `welcome` payload (consumed by the auto-update path).

## Test strategy

- **Unit:** `cargo test` — deterministic table-driven tests for protocol serde, approval policy, reconnect backoff, workspace resolve, config roundtrip.
- **Integration:** `tests/protocol_roundtrip.rs` — every client/server variant round-trips; router state machine invariants.
- **Manual QA** (per release): macOS arm64 + Linux x64 → first-run `configure` → `install` → `start` → TUI shows connected → synthetic run via `/api/runners/runs/` → approval prompt → decision.

## Release

Managed by `cargo-dist` (see `dist-workspace.toml` + `.github/workflows/release.yml`). Pushing a SemVer tag (e.g. `v0.1.0`) triggers the workflow, which builds binaries for macOS arm64 and Linux arm64/x64, generates the shell installer, and publishes everything to a GitHub Release.

To cut a release:

```bash
# bump version in runner/Cargo.toml, commit, then:
git tag v0.1.0
git push origin v0.1.0
```
