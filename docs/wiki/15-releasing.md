# 15 — Releasing

Pi Dash has two independent release streams:

1. **Runner** (`pidash` binary) — versioned, tag-driven, packaged by cargo-dist. **External contract** with installed users.
2. **Platform** (web + Django) — internal versioning; cloud and self-host follow their own paths.

## Runner releases

### Tooling

- `cargo-dist` — orchestrates the build matrix and packaging. Config in `dist-workspace.toml`.
- `.github/workflows/release.yml` — the GitHub Actions workflow, started by hand with the tag to create and a commit on `main`; it goes through the publication gate (`.github/publication-gate/README.md`).
- `runner/install.sh` / `runner/install.ps1` — opinionated wrapper installers (do auto-auth after install).
- The bare cargo-dist installers (`pidash-installer.sh`, `pidash-installer.ps1`, MSI) are also published — used by CI / base images that want to install without auto-auth.

### Build matrix

Each tagged release produces binaries for:

- macOS arm64 (Apple Silicon)
- macOS x86_64 (Intel)
- Linux arm64
- Linux x86_64
- Windows x86_64 (zip + MSI)

Plus shell, PowerShell, and MSI installer scripts.

### Tag & ship

```bash
# Bump runner/Cargo.toml to X.Y.Z and merge that to main, then:
gh workflow run Release --ref main -f tag=pidash-vX.Y.Z -f sha="$(git rev-parse origin/main)"
```

The workflow builds, finishes and scans the candidate. After a release owner approves the `release-publisher` environment, the publisher creates the tag and publishes the scanned files to GitHub Releases. Do not push release tags yourself: the tag ruleset refuses them, and no workflow starts on a tag push.

### Prereleases

Tags with a SemVer suffix (`-rc.1`, `-alpha.1`, `-beta.1`) are deliberately **excluded** from `/releases/latest/`. The public install one-liners always serve the last stable release. To install a prerelease, use the tag-pinned URL:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/download/pidash-vX.Y.Z-rc.1/install.sh | sh
```

### Announce the release to runners

The Django backend reads two env vars and folds them into every `welcome` session response (see [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md)):

| Env var                 | Effect                                                                                                       |
| ----------------------- | ------------------------------------------------------------------------------------------------------------ |
| `LATEST_RUNNER_VERSION` | Drives the yellow "update available" advisory. With auto-update on (default), triggers in-place binary swap. |
| `MIN_RUNNER_VERSION`    | Drives the red "update required" advisory.                                                                   |

For stable runner releases, `.github/workflows/release.yml` automatically writes the tag version to `/pidash/config/test/LATEST_RUNNER_VERSION` and `/pidash/config/prod/LATEST_RUNNER_VERSION` in SSM. It does **not** restart or redeploy the API; running containers pick up the value on their next normal restart/deploy. Leave `MIN_RUNNER_VERSION` alone unless you're forcing a floor (e.g. shedding an EOL wire version). Runners with auto-update disabled see a `pidash update --restart` hint so they can install and apply the release in one command.

For self-hosted or non-Cloud environments, leave both unset to skip the announcement entirely.

### In-place updates

`pidash` swaps its on-disk binary while running; the **running process is never disturbed**. The new binary takes effect on the next natural restart (`pidash restart`, host reboot, service-manager respawn after crash).

`pidash update --restart` swaps the binary and restarts the daemon in one command. `pidash update` without `--restart` only swaps the binary.

`pidash update` only works for installs done via cargo-dist installers (they leave a receipt). Source builds and `cargo install` builds get a clear "reinstall via the installer if you want self-update" error.

## Platform releases (web + Django)

The OSS platform (this repo) uses standard SemVer in `package.json` (currently `1.3.0`). The cut process is intentionally lightweight — see `RELEASING.md` at the repo root for the current procedure.

### Pi Dash Cloud (`private-pi-dash`)

The hosted Cloud overlay lives in a separate repo. Its release tags are **date-based**: `vYYYY.M.D` (no zero-pad), starting from 2026-05-24. Prior cloud releases used SemVer. This OSS repo does not produce those tags.

## Versioning summary

| Component               | Scheme                                               | Where tag lives                   |
| ----------------------- | ---------------------------------------------------- | --------------------------------- |
| Runner (`pidash`)       | SemVer with `pidash-v` prefix (e.g. `pidash-v0.1.4`) | This repo                         |
| Runner wire protocol    | Integer (currently `4`)                              | `runner/src/cloud/protocol.rs`    |
| Platform (web + Django) | SemVer, root `package.json`                          | This repo                         |
| Cloud overlay           | Date tags `vYYYY.M.D`                                | `private-pi-dash` (separate repo) |

## Where to read next

- `RELEASING.md` at the repo root — current platform release procedure
- `runner/README.md` — install one-liners, version pinning recipes, advisory states
- [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md) — how the version advisories propagate
