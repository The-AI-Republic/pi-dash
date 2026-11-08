# Rust platform releases

The Rust backend and new web frontend release together, independently of the
Python platform and runner CLI. `.github/workflows/release-rust-platform.yml`
publishes these Docker Hub repositories:

| Component                            | Image                             |
| ------------------------------------ | --------------------------------- |
| Rust API / worker                    | `airepublic/pi-dash-backend-rust` |
| New web frontend (community edition) | `airepublic/pi-dash-web-new-rust` |

Each version supports `linux/amd64` and `linux/arm64`. Both components use the
same version from the release tag; Cargo and npm package versions do not select
the release version. OCI image labels record the version and source commit.

## Trigger a release

Merge the workflow into the default branch before using GitHub's release UI.
Use a commit containing the workflow, release helper, and this runbook.

Push a tag at the commit you want to ship:

```sh
git tag rust-platform-v0.1.0-rc.1
git push origin rust-platform-v0.1.0-rc.1
```

For a final release, use `rust-platform-v0.1.0`. Versions must use
`vMAJOR.MINOR.PATCH`, optionally followed by a SemVer prerelease suffix such as
`-rc.1` or `-alpha-1`. Build metadata (`+...`) is not supported in Docker tags.

Alternatively, publish a GitHub Release against a `rust-platform-v...` tag.
The `release: published` trigger supports both final and prereleases. You can
also run **Release Rust Platform** manually with an existing tag to retry a
failed release. All paths resolve and build the tag's commit, including manual
runs dispatched from a different branch.

The tag-push path creates the GitHub Release after publishing the images.
Releases created with the workflow's `GITHUB_TOKEN` do not recursively trigger
another run. If you manually publish a release while its tag build is running,
both events can run sequentially; updates target the same tag and asset names.
Reusing or moving release tags is unsupported.

## Published outputs

- Native images are built and smoke-tested on each architecture before push.
  Backend checks exercise CLI startup, runtime libraries, data files, and shell
  syntax. Frontend checks exercise nginx and a client-side route fallback.
- After all four builds pass, multi-architecture `:vX.Y.Z` manifests are
  published. Final releases also update `:stable` and `:latest` in the two Rust
  image repositories. Prereleases update only their version tag. A GitHub
  Release marked prerelease is treated as a prerelease even without a suffix.
- The GitHub Release keeps the full `rust-platform-v...` tag and includes
  `images.json`, the Docker manifest JSON for each image (including platform
  digests), and this runbook. It is explicitly excluded from GitHub's global
  **Latest** designation, which existing Python installers depend on.
- Architecture staging tags include the full source commit. A failed build
  may leave staging images; it cannot promote version or channel manifests
  until every build passes. Registry pushes and GitHub publication are separate
  operations, so a publication failure may require rerunning the workflow.

The workflow uses the existing `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN`
secrets. The account must have push access to both new image repositories.
GitHub Release uploads use `GITHUB_TOKEN` with job-local `contents: write`.

Other release streams keep their existing triggers and image repositories:

| Stream                                                    | Tag prefix        | Workflow                    |
| --------------------------------------------------------- | ----------------- | --------------------------- |
| Python platform (including existing frontend coexistence) | `platform-v`      | `build-branch.yml`          |
| Rust backend + new web frontend                           | `rust-platform-v` | `release-rust-platform.yml` |
| Runner CLI                                                | `pidash-v`        | `release.yml`               |

## Deployment requirements

This release publishes component images; it does not deploy a server, replace
the existing community installer, or build an AIO image.

Use the versioned image references from the release's `images.json` together.
The frontend serves HTTP on port 3000 and uses same-origin API URLs. Route API
and auth requests to the backend before the frontend SPA fallback.

The backend serves port 8000 using its default API entrypoint. Worker roles use
`./bin/docker-entrypoint-worker.sh`; its queue is Postgres rather than AMQP.
Configure `DATABASE_URL`, a persistent `SECRET_KEY`, and the storage/cache
settings described in `rust-api/README.md`.

Schema migrations still require a compatible Python image running the Django
migrator. The Rust image contains no Python interpreter; its inherited migrator
entrypoint cannot run migrations. Select and pin that Python version explicitly
for your deployment; Rust and Python release version numbers are independent.

The Rust API also retains the Django cutover edge. Configure
`PIDASH_DJANGO_UPSTREAM` to a reachable compatible Django API, and enable the
appropriate `PIDASH_RUST_*` route groups deliberately. Those flags default off;
publishing this image does not switch traffic to Rust. Keep Django available for
fallback/static routes and validate your selected route groups and worker
configuration before deploying. See the cutover and ops sections of
`rust-api/README.md` for details. Release smoke tests do not establish full
application parity or database compatibility.
