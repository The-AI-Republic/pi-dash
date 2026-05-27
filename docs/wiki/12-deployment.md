# 12 — Deployment Topologies

Pi Dash ships three deploy paths plus a planned Kubernetes one. Pick by how much you want to manage yourself.

All paths require external **Postgres**, **Redis / Valkey**, **RabbitMQ**, and **S3-compatible** object storage — Pi Dash does not bundle data stores into the deploy artifacts.

| Path                                    | Best for                                | Trade-off                                                                                   |
| --------------------------------------- | --------------------------------------- | ------------------------------------------------------------------------------------------- |
| All-in-One (AIO) container              | Demos, homelab, evaluation, small teams | One container, internal `supervisord` — can't scale services independently                  |
| Docker Compose / Swarm                  | Anything beyond evaluation              | Real microservices stack: 6 service containers — more config, more flexibility              |
| Kubernetes / Helm                       | Production at scale                     | Helm chart **planned, not yet shipped** — see `deployments/kubernetes/community/README.md`  |
| Pi Dash Cloud (`pidash.airepublic.com`) | Don't want to run anything              | Hosted by the AI Republic team — see [pidash.airepublic.com](https://pidash.airepublic.com) |

## All-in-One Docker image

Folder: `deployments/aio/community/`.

A single container that bundles every Pi Dash service (web, admin, space, api, live, proxy), supervised internally by `supervisord`. Simplest path: a single `docker run`.

**You still need to provide:**

- Postgres
- Redis / Valkey
- RabbitMQ
- S3-compatible storage (e.g. MinIO)

Use cases: demos, homelab, evaluation, small teams. The internal `supervisord` keeps everything alive but you can't scale individual services — the whole container scales as one unit.

See `deployments/aio/community/README.md` for the exact `docker run` invocation and env vars.

## Docker Compose / Swarm self-hosting

Folder: `deployments/cli/community/` (Compose), `deployments/swarm/` (Swarm).

The full microservices stack: each app gets its own container, plus the data services. Independent scaling, rolling updates per service, easier debugging.

Services:

- `web` — main UI (React Router 7, served via nginx in prod)
- `admin` — instance admin UI
- `space` — public spaces
- `api` — Django + Channels backend
- `live` — Hocuspocus realtime server (see [10](./10-realtime-collaboration.md))
- `proxy` — Caddy reverse proxy (terminates TLS, fronts everything)

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
| `docker-compose.yml`       | Production-flavored (you'll typically copy + edit this)                                                                                                       |
| `docker-compose-local.yml` | **Local dev infra only** — Postgres + Redis + RabbitMQ + MinIO. `pnpm dev` attaches app processes to it. See [04 — Getting started](./04-getting-started.md). |

## Cloud overlay (`private-pi-dash`)

Pi Dash Cloud is the hosted SaaS. Its proprietary additions live in a separate repo (`private-pi-dash`) and are **not** part of this OSS repo. Cloud-only features are gated by the OIDC `plan` claim (see [11 — Authentication](./11-authentication.md)). Do not fork OSS files into the private repo — extend, don't duplicate.

## Where to read next

- [13 — Development workflow](./13-development-workflow.md) — local commands that mirror prod containers
- `deployments/*/README.md` — per-topology install instructions
- `apps/proxy/Caddyfile.*` — the production routing rules
