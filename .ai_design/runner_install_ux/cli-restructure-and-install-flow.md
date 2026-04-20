# Pi Dash Runner — CLI Restructure & Install Flow

Purpose:

- clean up the `pidash` CLI so service-lifecycle verbs (`start`/`stop`/`restart`/`status`) are top-level, consistent, and symmetric
- make `pidash install` the single install-time entry point and wire it to `configure` on fresh machines
- tighten the `config.toml` schema: required fields fail fast, optional fields default sensibly
- pin `runner.name` rules (charset, defaulting, per-workspace uniqueness) while keeping the cloud wire identifier (`runner_id`) as a stable UUID
- close the `pidash remove` crash-loop gap so removal is actually clean

Scope:

- `runner/` — CLI restructure, config validation, fresh-install gate, service-unit regeneration, `remove` cleanup
- `apps/api/` — registration endpoint accepts `runner.name`, validates charset, enforces `UNIQUE(workspace_id, runner_name)` and surfaces a typed collision error
- DB migration in the cloud for the composite unique constraint
- docs: install/operator guide refresh

Non-goals:

- changing what the daemon actually does while running (run assignment, Codex bridge, approvals, IPC, cloud protocol frames are all unchanged)
- introducing lazy-credential mode for the daemon — **reverted**, see §4
- Windows support, non-glibc Linux, non-systemd init systems
- changing `credentials.toml` shape; `runner_id` remains a UUID

Related docs:

- `.ai_design/implement_runner/runner-design.md`
- `.ai_design/implement_runner/operator-guide.md`
- `.ai_design/implement_runner/user-guide.md`
- `.ai_design/make_e2e_ready/implementation-plan.md`

## Terminology

One binary, two roles:

- **`pidash` (the CLI)** — the binary. Most invocations are fire-once, short-lived.
- **the runner / the daemon** — the one long-lived process that the CLI can spawn. Entered via the hidden `pidash __run` subcommand (replaces today's `pidash start` implementation). All other subcommands are either daemon-clients (talk to the runner over the Unix IPC socket) or standalone utilities (cloud HTTP API, local files).

There is exactly one daemon per machine. No other `pidash` subcommand runs long-lived.

## Background

Today's shape has three problems that motivate this redesign:

1. **Verb inconsistency.** `pidash start` is the foreground daemon; `pidash service start` is the OS-service start. `start` and `stop` live at different levels. Users encounter this immediately and it erodes trust.
2. **No fresh-install flow.** `pidash service install` writes the unit and `systemctl --user enable`s it even when there are no credentials. On next boot the service enters a crash loop. There is no interactive bridge from "install" to "configure."
3. **`pidash remove` leaves the service behind.** It deregisters from the cloud and wipes local files, but the installed unit stays. systemd/launchd then crash-loop on missing credentials.

Underneath that sit a few smaller but related gaps: `runner.name` has no defaulting or charset rules, the `config.toml` schema doesn't distinguish required from optional fields, and the release pipeline has no published binaries yet so the install story is source-build only.

## Decisions

### 1. User-visible command surface

| Command | Behavior |
|---|---|
| `pidash install` | Writes the OS service unit; on fresh install (no existing `config.toml`) with a TTY, chains into `pidash configure` interactively; enables and starts the service only after `configure` succeeds |
| `pidash uninstall` | Stops the service, disables it, deletes the unit file |
| `pidash start` | Starts the installed service (`systemctl --user start pidash` / `launchctl kickstart`) |
| `pidash stop` | Stops the installed service |
| `pidash restart` | Stop + start |
| `pidash status` | Service status (from systemctl/launchctl) **plus** runtime status (from IPC) in one output |
| `pidash configure` | Register with cloud; write `config.toml` + `credentials.toml` |
| `pidash remove` | Deregister from cloud, delete local config/creds, **and** uninstall the service |
| `pidash doctor` | Preflight checks (codex installed + logged in, git configured, cloud reachable) |
| `pidash tui` | Interactive UI attached to the running daemon over IPC |
| `pidash rotate` | Rotate the runner credential |
| `pidash issue` / `comment` / `state` / `resolve` / `workspace` | Cloud API clients (unchanged by this design) |
| `pidash __run` | **Hidden.** What systemd/launchd execs. Excluded from `--help` via `#[command(hide = true)]`. Not a supported user-facing verb |

Dropped: the `pidash service <subcommand>` subgroup disappears entirely. No deprecated alias — no releases have shipped, so nothing to break.

Rationale:

- `start`/`stop`/`restart`/`status` are all service-lifecycle verbs at the same level. Symmetric.
- Users never see or type the foreground daemon command. It still exists as plumbing because systemd (`Type=simple`) and launchd both require a foreground executable to supervise — we can't remove that code path, only hide it.
- Naming it `__run` (double-underscore prefix) telegraphs "internal" when it shows up in unit files and ps output.

### 2. Fresh-install gate

`pidash install` behavior:

1. Writes the service unit (systemd user unit on Linux, LaunchAgent on macOS) — but does **not** enable or start it yet.
2. Determines `fresh = !config.toml.exists() || !config.toml.is_valid()`.
3. If `fresh` and stdin is a TTY and `--no-configure` was not passed → chain into `pidash configure` interactively.
4. If `configure` succeeds (or config was already valid) → enable + start the service.
5. If `fresh` and non-TTY (CI, ansible, Docker build) or `--no-configure` → print next-step hint, exit without enabling. Nothing auto-starts a runner that isn't configured.

Net effect: three unambiguous phases.

- **Install** (interactive, one-time).
- **Start** (unattended, strict config, creds required).
- **Runtime** (the IPC/cloud/run loop).

The daemon never runs in a half-configured state.

### 3. `config.toml` schema — required vs optional

Strict required fields (no serde default, daemon start fails fast if missing):

- `runner.cloud_url`
- `workspace.working_dir`
- `codex.binary`

Optional (with defaults):

- `runner.name` — defaulted, see §5
- `runner.workspace_slug` — optional; UX subcommands (`issue`, `comment`, etc.) surface a "rerun `pidash configure`" error when missing (existing behavior, preserved)
- `approval_policy.*` — existing defaults
- `logging.*` — existing defaults

When `__run` loads a config missing a required field, it exits immediately with a message pointing at `pidash configure`. No partial boot, no silent degradation.

### 4. Credentials at start — required

Earlier iteration considered making credentials lazy (run IPC and sit in `Unregistered` state). **Reverted.**

Reasons:

- The fresh-install gate (§2) guarantees `configure` runs before the service is enabled, so the daemon never sees missing creds in the normal flow.
- Lazy mode would add an `Unregistered` state to IPC, status, TUI, and tests — surface area for little benefit.
- Manual deletion of `credentials.toml` is a self-inflicted wound. Fast-failing with a log pointing at `pidash configure` is the right signal; a silent idle runner is not.
- `pidash remove` now stops + uninstalls the service (see §6), so the "deregistered but service still installed → crash loop" scenario goes away at its root.

`__run` keeps the eager `load_all(paths)?` call but with a better error message:

```
error: no credentials found at ~/.config/pidash/credentials.toml
hint:  run `pidash configure` to register this runner, or `pidash install` to set up the service for the first time
```

### 5. `runner.name` rules

**Charset.** `[A-Za-z0-9_-]` only. No spaces. No other characters. Validated on both client (reject bad `--name` input in `configure`) and server (defense in depth).

**Default.** When `configure` is invoked without an explicit `--name`, generate `pidash_runner_<3 random chars from [A-Za-z0-9]>`. 62³ ≈ 238k names — ample for the per-workspace collision domain (see below).

**Uniqueness scope — per workspace, not global.** The cloud DB enforces `UNIQUE(workspace_id, runner_name)`. `my-laptop` in workspace A can coexist with `my-laptop` in workspace B.

**Internal identity stays UUID.** `credentials.toml::runner_id` remains a server-issued UUID used for WebSocket auth (`X-Runner-Id` header). `runner.name` is the human handle shown in the TUI and cloud UI. Keeping both means:

- Workspace renames don't invalidate runner identity.
- Name renames (if we ever add that) don't require re-issuing credentials.
- Storage is clean: `runners.id uuid PK`, `runners.name text`, `UNIQUE(workspace_id, name)`.

**Collision handling.**

- Explicit `--name foo` that collides with an existing runner in the same workspace → loud error, exit non-zero. Do not silently rename user-provided input.
- Auto-generated default that collides → retry with a fresh random suffix up to 5 times, then error. 62³ with ≤5 retries gives a negligible failure probability even at thousands of runners per workspace.

Wire shape of the register endpoint (`apps/api/pi_dash/runner/views/register.py`): accepts `name` in the payload, validates charset, returns `409 Conflict` with `{"error": "runner_name_taken"}` on collision so the runner can decide whether to retry (default case) or surface the error (explicit `--name` case).

### 6. `pidash remove` cleanup extension

Current `remove` (`runner/src/cli/remove.rs`):

1. POSTs deregister to cloud (skipped with `--local-only`).
2. Deletes `config.toml` and `credentials.toml`.

Extended `remove`:

1. Stops the service (`svc.stop()`, tolerant of "not running").
2. Uninstalls the service unit (`svc.uninstall()`, tolerant of "not installed").
3. Deregisters from cloud (unchanged, `--local-only` still skips).
4. Deletes `config.toml` and `credentials.toml`.

Ordering: service-first so the daemon isn't still talking to the cloud when we tell the cloud to forget it, and isn't holding the IPC socket when we delete local state.

This closes the crash-loop hole — after `pidash remove`, the machine is clean.

### 7. Auto-start on reboot

No code change to the unit generators — they already set the right restart policy. Documentation change only.

- **macOS**: existing LaunchAgent has `RunAtLoad=true` + `KeepAlive=true`. Starts at user login, restarts on crash. No extra step needed. A system-boot-before-login start would require a LaunchDaemon — **not in scope**.
- **Linux**: existing systemd user unit has `Restart=on-failure` and is `enable`d. Starts at user login by default. For **boot-before-login** auto-start, the user must run `sudo loginctl enable-linger $USER` once. `pidash install` prints this hint after a successful install, but does not run `sudo` itself.

### 8. Supported OS matrix (documentation truth)

Officially (per `runner/dist-workspace.toml`):

| OS | Arch | Service backend |
|---|---|---|
| macOS | aarch64 (Apple Silicon) | launchd |
| macOS | x86_64 (Intel) | launchd |
| Linux | x86_64 (glibc) | systemd (user) |

Build-from-source also works on Unix-likes with Rust 1.93 when the init system is systemd or launchd. Not supported: Windows, musl, BSDs, non-systemd Linux inits.

## Implementation plan

Three PRs, reviewable independently:

### PR 1 — CLI restructure (runner only, no cloud changes)

- [ ] Add top-level subcommands: `install`, `uninstall`, `start`, `stop`, `restart`, `status`
- [ ] Add hidden `__run` subcommand (`#[command(hide = true)]`), move current `start.rs` body into it
- [ ] Remove the `service` subcommand group
- [ ] Regenerate unit files to call `{exe} __run` instead of `{exe} start`
- [ ] Merge service-level + IPC `status` output into a single command
- [ ] Update `runner/README.md` command examples
- [ ] Contract tests: each new top-level verb exists, `service` subgroup is gone, `__run` is hidden from `--help`

### PR 2 — Install flow + config strictness + remove cleanup

- [ ] `pidash install` gates enable/start on config validity; TTY → chains into `configure`; non-TTY → prints next-step hint
- [ ] `--no-configure` flag on `install`
- [ ] Required-field validation on `Config` deserialization; clear error message from `__run` on missing field
- [ ] Helpful error on missing credentials at `__run` entry, pointing at `pidash configure`
- [ ] `pidash remove` extended to stop + uninstall service before deleting local state
- [ ] Print `loginctl enable-linger` hint on successful Linux install
- [ ] Integration test: fresh install + configure + start + status + remove cycle on a tmpdir config

### PR 3 — `runner.name` rules + cloud uniqueness (coordinated runner + cloud)

Runner side:

- [ ] Charset validator on `runner.name` (client-side rejection of invalid `--name foo`)
- [ ] Default generator: `pidash_runner_<3 chars [A-Za-z0-9]>`
- [ ] `configure` retries up to 5 times on `409 runner_name_taken` when the name was auto-generated; fails loudly when the name was user-supplied

Cloud side (`apps/api/`):

- [ ] DB migration: add `name text not null` to runners table, backfill from existing UUID-based identifiers, add `UNIQUE(workspace_id, name)` constraint
- [ ] Register endpoint (`runner/views/register.py`): accept `name`, validate charset, return `409 {"error": "runner_name_taken"}` on conflict
- [ ] Include `name` in runner listings and workspace-admin UI surfaces
- [ ] Contract tests: charset validation, collision returns 409, happy path returns the created runner with `name` echoed back

## Migration / upgrade path

There are no public releases yet, so the only population we need to worry about is internal dogfooding installs.

For anyone with the current `ExecStart={exe} start` unit installed:

```
pidash uninstall     # old binary still works, removes the stale unit
# pull, rebuild, reinstall
pidash install
```

We do not attempt in-place unit rewriting from the new binary. Users run `uninstall` with the old binary first because the new `pidash start` means "start the service," which would infinite-loop against the old unit. `pidash install` from the new binary writes the correct `ExecStart={exe} __run` line.

Release notes for whatever first-published version bundles these changes must call out this two-step upgrade explicitly.

## Testing

Runner:

- **Unit.** Charset validator for `runner.name`, required-field validation on `Config`, random-name generator distribution, retry-on-conflict loop.
- **Integration.** `tests/pidash_cli_contract.rs` extended: presence of new top-level verbs, `__run` hidden from help, `service` subgroup absent.
- **Manual QA.** Fresh install on a scratch macOS machine + a scratch Linux VM: install → chained configure → service running → `status` shows connected → `remove` leaves machine clean.

Cloud:

- **Unit.** Charset validator on the API side, 409 path on duplicate.
- **Contract.** Existing `apps/api/tests/contract/runner/test_registration.py` extended for `name` payload handling and the collision response.
- **Migration.** Dry-run against a snapshot of the staging DB to confirm backfill of the new `name` column and uniqueness constraint.

## Out of scope / deferred

- **LaunchDaemon (macOS boot-before-login) support.** Today's LaunchAgent starts at user login, which is fine for a dev machine. Revisit if a headless-Mac-as-runner use case emerges.
- **Non-systemd Linux inits (OpenRC, runit, s6).** Not in the release matrix.
- **Windows.** Would need a Windows service backend, IPC over named pipes, and removal of the `nix` dependency. Separate project.
- **Hot-reload of credentials without a service restart.** Possible via a file-watcher on `credentials.toml`, but not worth the complexity given how rarely creds rotate.
- **Renaming a runner after initial `configure`.** Out of scope; would need a cloud-side rename endpoint and UUID-based identity is already doing the heavy lifting.
