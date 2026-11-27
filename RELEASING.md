# Releasing Pi Dash

Pi Dash ships two independently-versioned components, each with its own
release pipeline. Both go through the publication gate
(`.github/publication-gate/README.md`): you start a workflow with a commit
that is already on `main`, CI builds, finishes, scans and tests the candidate,
and after a release owner approves the `release-publisher` environment the
publisher creates the tag and publishes exactly what was scanned. Do not push
release tags yourself; the tag ruleset refuses them, and a tag push no longer
starts any workflow.

## Tag scheme

| Component                                          | Tag prefix           | Workflow (`workflow_dispatch`)                         | Builds                                                                                                                                                                    |
| -------------------------------------------------- | -------------------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Runner CLI** (the `pidash` command)              | `pidash-v<semver>`   | `.github/workflows/release.yml` (cargo-dist)           | Cross-platform binaries (macOS arm64, Linux arm64/x86_64), Homebrew formula, GitHub Release with installer scripts                                                        |
| **Pi Dash platform** (the 6 service Docker images) | `v<semver>`          | `.github/workflows/build-branch.yml` (Branch Build CE) | Multi-arch Docker images pushed to `airepublic/pi-dash-{frontend,backend,space,admin,live,proxy}`, GitHub Release with `setup.sh` + `docker-compose.yml` for self-hosters |

Versions are independent. The runner is currently `0.1.x`; the platform
follows `package.json` (`1.x.x`). Don't try to align them — each component
gets a fix or feature on its own cadence.

## Cutting a runner release

1. Land all the runner-related PRs on `main`.
2. Bump the version in `runner/Cargo.toml`:
   ```toml
   [package]
   name = "pidash"
   version = "0.1.2"   # was 0.1.1
   ```
3. Land the bump on `main` through a pull request, then start the release
   with the merged commit:
   ```bash
   git fetch origin
   gh workflow run Release --ref main \
     -f tag=pidash-v0.1.2 \
     -f sha="$(git rev-parse origin/main)"
   gh run watch
   ```
4. CI builds, finishes and scans the candidate, then waits for a release
   owner to approve the `release-publisher` environment. The publisher then
   creates `pidash-v0.1.2` at that commit and publishes the scanned files.
5. When green, the GitHub Release will be at
   `https://github.com/The-AI-Republic/pi-dash/releases/tag/pidash-v0.1.2`
   with a `pidash-installer.sh` and platform-specific archives.
6. The release workflow automatically writes
   `LATEST_RUNNER_VERSION=0.1.2` to the Cloud SSM parameters for test and
   prod. It does not restart or redeploy the API; the currently running
   containers pick the value up on their next normal restart/deploy. Set
   `MIN_RUNNER_VERSION=0.1.2` manually only for a breaking-protocol or
   security release. Runners with `auto_update` enabled — the default —
   will swap the on-disk binary on their next session bootstrap and surface
   a "restart to apply" advisory in `pidash status` / the TUI. Runners
   with `auto_update` disabled show an "update available — run
   `pidash update --restart`" banner instead. See `runner/README.md`
   (Auto-update) for the full rendering matrix.

### The built-in engine in the release

Every runner release archive and the MSI carry the built-in agent engine
(OpenAI Codex, renamed `pidash-agent-engine`) under `engine/<version>/`.
The pinned upstream release is the one line in `runner/ENGINE_VERSION`
(e.g. `rust-v0.153.4`); the desktop build scripts read the same file. The
SHA-256 of each upstream asset for that tag is pinned in
`runner/ENGINE_SHA256`. To move to a new Codex release, change both files in
one PR (the comment at the top of `ENGINE_SHA256` says how to get the
digests). The release workflow's "Stage built-in engine" step
(`runner/scripts/stage_engine.py`) fetches the asset per target and refuses it
unless it matches the pinned digest and the one in the upstream release
metadata. The step needs `gh` with `GH_TOKEN`, which the workflow already
provides.

## Cutting a platform release

1. Land all platform-related PRs on `main`.
2. Bump the version in the root `package.json`:
   ```json
   {
     "version": "1.4.0"
   }
   ```
3. Land the bump on `main` through a pull request, then start the build
   with the merged commit:
   ```bash
   git fetch origin
   gh workflow run "Branch Build CE" --ref main \
     -f sha="$(git rev-parse origin/main)" \
     -f build_type=Release \
     -f releaseVersion=v1.4.0 \
     -f isPrerelease=false
   ```
4. CI does the rest:
   - Builds the Docker images in parallel (multi-arch via QEMU) into
     archives, and scans them with the self-host assets
   - After a release owner approves `release-publisher`, pushes the scanned
     images to Docker Hub with three tags each: `:v1.4.0`, `:stable`, `:latest`
   - Builds the AIO image (if the `airepublic/pi-dash-aio-community` repo
     exists; CI will fail this single job otherwise without affecting the
     other 6)
   - Creates the `v1.4.0` tag and a GitHub Release with the platform `setup.sh` and current
     `docker-compose.yml` so self-hosters can `curl … | bash` install

## Prereleases

Suffix the version with anything starting with `-` followed by a letter,
for example `-f tag=pidash-v0.1.2-rc.1` or `-f releaseVersion=v1.4.0-beta.2`.

Both pipelines treat any `-<alpha>` suffix as a prerelease — the GitHub
Release is marked as a prerelease, and Docker images skip the `:stable` /
`:latest` tags (they only get the explicit version tag).

## What you need set up before the first real release

First, the publication gate's one-time setup in
`.github/publication-gate/README.md` (publisher App, environments, tag
ruleset). Every credential below is a secret of the `release-publisher`
environment, not a repository secret.

For runner release advisories:

1. **AWS SSM credentials in the `release-publisher` environment**
   - `AWS_ACCESS_KEY_ID`
   - `AWS_SECRET_ACCESS_KEY`
   - IAM permission: `ssm:PutParameter` on
     `/pidash/config/test/LATEST_RUNNER_VERSION` and
     `/pidash/config/prod/LATEST_RUNNER_VERSION`.
   - The workflow intentionally does not restart EC2 or redeploy the API.

For platform releases (cargo-dist runner releases already work):

1. **Docker Hub credentials in the `release-publisher` environment**
   - `DOCKERHUB_USERNAME`
   - `DOCKERHUB_TOKEN` — Personal Access Token with Read & Write on
     `airepublic/*` repos
   - Set at: <https://github.com/The-AI-Republic/pi-dash/settings/environments>

2. **The 6 Docker Hub repos must exist** under the `airepublic` namespace:
   `pi-dash-frontend`, `pi-dash-backend`, `pi-dash-space`, `pi-dash-admin`,
   `pi-dash-live`, `pi-dash-proxy`. Use `scripts/register-dockerhub-repos.sh`
   if you need to recreate any.

## Smoke-testing the platform pipeline before tagging

Before the first platform release, run a non-release build to validate the
workflow still works:

```bash
gh workflow run "Branch Build CE" \
  --ref main \
  -f sha="$(git rev-parse origin/main)" \
  -f build_type=Build \
  -f releaseVersion=v0.0.0 \
  -f isPrerelease=false \
  -f arm64=false \
  -f aio_build=false
gh run watch
```

This builds and scans the images and, once `release-publisher` is approved,
pushes dev-tagged images to `airepublic/*:main`. If it works, releases will
too.

## Rolling back a release

Docker Hub:

```bash
# Re-tag the previous version's manifest as :latest
docker buildx imagetools create -t airepublic/pi-dash-frontend:latest \
  airepublic/pi-dash-frontend:v1.3.0
# Repeat for each of the 6 images.
```

GitHub Release: edit the release page, mark as draft, or delete entirely.
