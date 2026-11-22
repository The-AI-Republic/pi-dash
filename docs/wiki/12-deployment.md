# 12 — Deployment Topologies

Pi Dash ships three deploy paths plus a planned Kubernetes one. Pick by how much you want to manage yourself.

All paths require external **Postgres**, **Redis / Valkey**, and **S3-compatible** object storage — Pi Dash does not bundle data stores into the deploy artifacts. (The active system needs no RabbitMQ: background jobs run on the Postgres queue.)

| Path                                    | Best for                                | Trade-off                                                                                            |
| --------------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| All-in-One (AIO) container              | Demos, homelab, evaluation, small teams | One container, internal `supervisord` — can't scale services independently                           |
| Docker Compose / Swarm                  | Anything beyond evaluation              | Real microservices stack: Rust API + worker, `web_new`, `live`, proxy — more config, more flexibility |
| Kubernetes / Helm                       | Production at scale                     | Helm chart **planned, not yet shipped** — see `deployments/kubernetes/community/README.md`  |
| Pi Dash Cloud (`pidash.airepublic.com`) | Don't want to run anything              | Hosted by the AI Republic team — see [pidash.airepublic.com](https://pidash.airepublic.com) |

## All-in-One Docker image

Folder: `deployments/aio/community/`.

A single container that bundles every Pi Dash service (Rust `api` + `worker`, `web_new`, `live`, proxy), supervised internally by `supervisord`. Simplest path: a single `docker run`.

Database migration is an install step that runs once, before any service starts: the installer runs the `migrator` first and stops with a nonzero exit on failure. Services never run migrations; they only run the read-only schema check and exit nonzero when the schema is not current.

**You still need to provide:**

- Postgres
- Redis / Valkey
- S3-compatible storage (e.g. MinIO)

Use cases: demos, homelab, evaluation, small teams. The internal `supervisord` keeps everything alive but you can't scale individual services — the whole container scales as one unit.

See `deployments/aio/community/README.md` for the exact `docker run` invocation and env vars.

## Docker Compose / Swarm self-hosting

Folder: `deployments/cli/community/` (Compose), `deployments/swarm/` (Swarm).

The full microservices stack: each service gets its own container, plus the data services. Independent scaling, rolling updates per service, easier debugging.

Services:

- `api` — Rust API (`pidash-api serve`, port 8000) + `worker` (Postgres job queue)
- `web_new` — new React frontend (port 3000 behind Caddy)
- `live` — Hocuspocus realtime server (see [10](./10-realtime-collaboration.md))
- `proxy` — Caddy reverse proxy (terminates TLS, fronts everything)

Migration is an install/upgrade step: `docker compose run --rm migrator` (or the installer equivalent) runs once before any service starts and fails the install on error. `api` and `worker` start only after the migrator completes successfully; they never migrate on boot, only fail fast on a stale schema.

Recommended for anything beyond evaluation.

See `deployments/cli/community/README.md` for the Compose file walkthrough.

## Kubernetes / Helm

Folder: `deployments/kubernetes/community/`.

Helm chart publishing is **planned but not yet shipped**. Raw manifests / kustomize bases are in the folder; treat them as a starting point rather than a turnkey deploy. See the folder's README for current status.

## Production reverse proxy: Caddy

`apps/proxy/` ships Caddyfiles for both topologies:

- `Caddyfile.aio.ce` — AIO single-container
- `Caddyfile.ce` — full Compose / Swarm

Caddy handles TLS termination automatically (Let's Encrypt). It is **not** part of the pnpm workspace.

## docker-compose files at the repo root

| File                       | Purpose                                                                                                                                                       |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `docker-compose.yml`       | Self-host stack: Caddy proxy + Rust `api`/`worker` + `web_new` + `live` over external Postgres/Valkey/S3 (you'll typically copy + edit this) |
| `docker-compose-local.yml` | **Local dev** — Postgres + Valkey + MinIO + one-off `migrator`; the Rust API, `live` and `web_new` run on your host. See [04 — Getting started](./04-getting-started.md). |

## Cloud overlay (`private-pi-dash`)

Pi Dash Cloud is the hosted SaaS. Its proprietary additions live in a separate repo (`private-pi-dash`) and are **not** part of this OSS repo. Cloud-only features are gated by the OIDC `plan` claim (see [11 — Authentication](./11-authentication.md)). Do not fork OSS files into the private repo — extend, don't duplicate.

## Where to read next

- [13 — Development workflow](./13-development-workflow.md) — local commands that mirror prod containers
- `deployments/*/README.md` — per-topology install instructions
- `apps/proxy/Caddyfile.*` — the production routing rules
