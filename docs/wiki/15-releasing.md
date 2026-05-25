# 15 — Releasing

Pi Dash has two independent release streams:

1. **Runner** (`pidash` binary) — versioned, tag-driven, packaged by cargo-dist. **External contract** with installed users.
2. **Platform** (web + Django) — internal versioning; cloud and self-host follow their own paths.

## Runner releases

### Tooling

- `cargo-dist` — orchestrates the build matrix and packaging. Config in `dist-workspace.toml`.
- `.github/workflows/release.yml` — the GitHub Actions workflow triggered by SemVer tags.
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
# In runner/Cargo.toml, bump version to X.Y.Z, commit.
git tag pidash-vX.Y.Z          # tag prefix matters — workflow filters on it
git push origin pidash-vX.Y.Z
```

The workflow builds, signs, and publishes to GitHub Releases.

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

After cutting a runner release, set `LATEST_RUNNER_VERSION` on the cloud so opted-in runners auto-update. Leave `MIN_RUNNER_VERSION` alone unless you're forcing a floor (e.g. shedding an EOL wire version).

Leave both unset to skip the announcement entirely.

### In-place updates

`pidash` swaps its on-disk binary while running; the **running process is never disturbed**. The new binary takes effect on the next natural restart (`pidash restart`, host reboot, service-manager respawn after crash).

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
