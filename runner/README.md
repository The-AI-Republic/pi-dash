# Apple Pi Dash Runner

Local daemon + TUI that connects a developer machine to the Apple Pi Dash cloud and drives `codex app-server` for assigned tasks.

See `.ai_design/implement_runner/` for the design documents:

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
│   ├── cli/                  # clap subcommands: configure / start / service / status / tui / doctor / remove
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

## Commands

```bash
cargo build                                  # debug build
cargo test                                   # unit + integration tests
cargo check                                  # quick type-check
cargo clippy -- -D warnings                  # lint

./target/debug/apple-pi-dash-runner configure \
  --url https://cloud.apple-pi-dash.so \
  --token <ONE_TIME_CODE> \
  --name my-laptop

./target/debug/apple-pi-dash-runner service install
./target/debug/apple-pi-dash-runner service start
./target/debug/apple-pi-dash-runner tui
```

## Runtime paths (XDG)

- Config: `~/.config/apple-pi-dash-runner/`
- Data / logs: `~/.local/share/apple-pi-dash-runner/`
- Runtime dir: `$XDG_RUNTIME_DIR/apple-pi-dash-runner/` (Unix socket, PID file)

All secrets on disk are written with `0600`. The Unix IPC socket is also `0600`.

## Protocol

Wire version is `1` — bumped on incompatible shape changes. See `src/cloud/protocol.rs` for exhaustive schemas. Runner authenticates to the cloud with an HTTP `Authorization: Bearer <runner_secret>` header on the WebSocket upgrade request and echoes its UUID in `X-Runner-Id`. The server echoes an accepted `protocol_version` in the `welcome` frame.

## Test strategy

- **Unit:** `cargo test` — deterministic table-driven tests for protocol serde, approval policy, reconnect backoff, workspace resolve, config roundtrip.
- **Integration:** `tests/protocol_roundtrip.rs` — every client/server variant round-trips; router state machine invariants.
- **Manual QA** (per release): macOS arm64/x64 + Linux x64 → first-run `configure` → `service install` → TUI shows connected → synthetic run via `/api/runners/runs/` → approval prompt → decision.

## Release

Managed by `cargo-dist` (see `dist-workspace.toml`). CI matrix builds signed binaries for macOS arm64/x64 and Linux x64, plus a Homebrew formula.
