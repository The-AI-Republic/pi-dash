# Releasing Pi Dash

Pi Dash ships two independently-versioned components, each with its own
release pipeline. Tag the right prefix from `main` and CI takes it from there.

## Tag scheme

| Component                                          | Tag prefix           | Triggers                                               | Builds                                                                                                                                                                    |
| -------------------------------------------------- | -------------------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Runner CLI** (the `pidash` command)              | `pidash-v<semver>`   | `.github/workflows/release.yml` (cargo-dist)           | Cross-platform binaries (macOS arm64, Linux arm64/x86_64), Homebrew formula, GitHub Release with installer scripts                                                        |
| **Pi Dash platform** (the 6 service Docker images) | `platform-v<semver>` | `.github/workflows/build-branch.yml` (Branch Build CE) | Multi-arch Docker images pushed to `airepublic/pi-dash-{frontend,backend,space,admin,live,proxy}`, GitHub Release with `setup.sh` + `docker-compose.yml` for self-hosters |

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
3. Commit, tag, push:
   ```bash
   git checkout main && git pull
   # edit runner/Cargo.toml ...
   git commit -am "chore(runner): bump version to 0.1.2"
   git tag pidash-v0.1.2
   git push origin main pidash-v0.1.2
   ```
4. CI does the rest. Watch:
   ```bash
   gh run watch
   ```
5. When green, the GitHub Release will be at
   `https://github.com/The-AI-Republic/pi-dash/releases/tag/pidash-v0.1.2`
   with a `pidash-installer.sh` and platform-specific archives.

## Cutting a platform release

1. Land all platform-related PRs on `main`.
2. Bump the version in the root `package.json`:
   ```json
   {
     "version": "1.4.0"
   }
   ```
3. Commit, tag, push:
   ```bash
   git checkout main && git pull
   # edit package.json ...
   git commit -am "chore(platform): bump version to 1.4.0"
   git tag platform-v1.4.0
   git push origin main platform-v1.4.0
   ```
4. CI does the rest:
   - Builds 6 Docker images in parallel (multi-arch via QEMU)
   - Pushes them to Docker Hub with three tags each: `:v1.4.0`, `:stable`, `:latest`
   - Builds the AIO image (if the `airepublic/pi-dash-aio-community` repo
     exists; CI will fail this single job otherwise without affecting the
     other 6)
   - Creates a GitHub Release with the platform `setup.sh` and current
     `docker-compose.yml` so self-hosters can `curl … | bash` install

## Prereleases

Suffix the version with anything starting with `-` followed by a letter:

```bash
git tag pidash-v0.1.2-rc.1
# or
git tag platform-v1.4.0-beta.2
```

Both pipelines treat any `-<alpha>` suffix as a prerelease — the GitHub
Release is marked as a prerelease, and Docker images skip the `:stable` /
`:latest` tags (they only get the explicit version tag).

## What you need set up before the first real release

For platform releases (cargo-dist runner releases already work):

1. **Docker Hub credentials in GitHub Actions secrets**
   - `DOCKERHUB_USERNAME`
   - `DOCKERHUB_TOKEN` — Personal Access Token with Read & Write on
     `airepublic/*` repos
   - Set at: <https://github.com/The-AI-Republic/pi-dash/settings/secrets/actions>

2. **The 6 Docker Hub repos must exist** under the `airepublic` namespace:
   `pi-dash-frontend`, `pi-dash-backend`, `pi-dash-space`, `pi-dash-admin`,
   `pi-dash-live`, `pi-dash-proxy`. Use `scripts/register-dockerhub-repos.sh`
   if you need to recreate any.

## Smoke-testing the platform pipeline before tagging

Before pushing the very first `platform-v*` tag, run a non-release build
manually to validate the workflow still works:

```bash
gh workflow run "Branch Build CE" \
  --ref main \
  -f build_type=Build \
  -f releaseVersion=v0.0.0 \
  -f isPrerelease=false \
  -f arm64=false \
  -f aio_build=false
gh run watch
```

This builds and pushes 6 dev-tagged images to `airepublic/*:main`. If it
works, real tag-triggered releases will too.

## Rolling back a release

Docker Hub:

```bash
# Re-tag the previous version's manifest as :latest
docker buildx imagetools create -t airepublic/pi-dash-frontend:latest \
  airepublic/pi-dash-frontend:v1.3.0
# Repeat for each of the 6 images.
```

GitHub Release: edit the release page, mark as draft, or delete entirely.
