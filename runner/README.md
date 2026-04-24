# Pi Dash Runner

Local daemon + TUI (`pidash` binary) that connects a developer machine to the Pi Dash cloud and drives `codex app-server` for assigned tasks.

## Install

Prebuilt binaries for macOS (arm64) and Linux (arm64, x86_64) are published to GitHub Releases. The one-liner below downloads the installer, verifies checksums, and drops `pidash` into `$HOME/.local/bin`:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.sh | sh
```

Pin to a specific version instead of `latest` by swapping in the tag, e.g. `.../releases/download/v0.1.0/pidash-installer.sh`.

**Prerequisite:** the runner shells out to [Codex](https://github.com/openai/codex) — install it and make sure `codex --version` works before running `pidash configure`. `pidash doctor` checks this.

After install:

```bash
pidash configure \
  --url http://localhost \
  --token <ONE_TIME_CODE> \
  --name my-laptop
pidash install   # register a systemd user unit (Linux) or launchd agent (macOS)
pidash start
pidash tui       # optional: open the interactive UI
```

Generate the one-time token from the Pi Dash web UI under the runners admin page.

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

Wire version is `1` — bumped on incompatible shape changes. See `src/cloud/protocol.rs` for exhaustive schemas. Runner authenticates to the cloud with an HTTP `Authorization: Bearer <runner_secret>` header on the WebSocket upgrade request and echoes its UUID in `X-Runner-Id`. The server echoes an accepted `protocol_version` in the `welcome` frame.

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
