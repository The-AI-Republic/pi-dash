# 01 — Architecture Overview

Pi Dash is an **AI agent orchestration platform** built for "As Coding" (asynchronous vibe coding): you define what needs to be built, agents implement it in the background, and you spend your time scoping and reviewing rather than watching terminals.

The product is the orchestration layer. It does **not** ship its own coding agent — you bring `claude` or `codex`, and Pi Dash drives them.

## The three components

```
┌───────────────────────────┐         ┌────────────────────────────┐
│   Pi Dash Platform        │         │    Pi Dash Runner          │
│   (cloud / self-hosted)   │  ◀─────▶│    (on your dev machine)   │
│                           │  HTTPS  │                            │
│ • Web UI (React Router 7) │  + WS   │ • Rust daemon (`pidash`)   │
│ • Django REST API         │         │ • Supervisor + state mach. │
│ • Channels / WebSockets   │         │ • Codex / Claude bridge    │
│ • Celery (background)     │         │ • Local TUI + IPC socket   │
│ • Live editor (Hocuspocus)│         │                            │
└───────────────────────────┘         └─────────────┬──────────────┘
                                                    │ subprocess
                                                    ▼
                                      ┌────────────────────────────┐
                                      │   AI Agent (BYO)           │
                                      │                            │
                                      │ • `claude`  (Anthropic)    │
                                      │ • `codex`   (OpenAI)       │
                                      └────────────────────────────┘
```

### 1. Pi Dash Platform (cloud or self-hosted)

The orchestration hub: work items, cycles, modules, views, pages, analytics — plus the cloud side of the runner protocol. Lives under `apps/` and the Django backend in `apps/api/`.

See [05 — Frontend architecture](./05-frontend-architecture.md) and [06 — Backend architecture](./06-backend-architecture.md).

### 2. Pi Dash Runner

A single Rust binary (`pidash`) installed on a developer machine. It authenticates against the platform, polls for assigned tasks, dispatches each one to the user's configured AI agent as a subprocess, and reports results back. Lives under `runner/`.

See [07 — Runner architecture](./07-runner-architecture.md) and [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md).

### 3. AI Agent (user-provided)

Pi Dash is agent-agnostic. The runner ships first-class support for **Claude Code** (`claude`) and **Codex** (`codex`); the dispatch layer is designed so other agents can be wired in without changing the orchestration model. Users install the agent binary themselves; `pidash doctor` checks it.

## End-to-end task lifecycle

1. **User creates a work item** in the web UI (`apps/web`).
2. **Platform assigns** the work item to a project that has at least one connected runner.
3. **Orchestration layer** (`apps/api/pi_dash/orchestration/`) produces an agent run plan — phase, prompt, working dir, approval policy — and persists it.
4. **Runner polls** the cloud for new runs over HTTPS long-poll (wire protocol v4, see [08](./08-cloud-runner-protocol.md)).
5. **Runner spawns** the configured agent (`codex app-server` or `claude`) in the project's workspace, streaming the JSON-RPC bridge into the daemon.
6. **Approval router** (first-writer-wins between TUI / cloud / policy) handles any approval prompts the agent emits.
7. **Run transcripts** (JSONL) are written locally under the runner's data dir, and progress events are pushed back to the platform.
8. **User reviews** results in the web UI — diff, transcript, approval decisions, follow-up.

## The three seams to know

When a change spans more than one layer, you almost always cross one of these:

| Seam                                                         | Files                                                            | Versioned?                                                             |
| ------------------------------------------------------------ | ---------------------------------------------------------------- | ---------------------------------------------------------------------- |
| **REST API** (browser ↔ Django)                              | `apps/api/pi_dash/{app,api}/urls/` + `packages/services/` client | No (internal, deploy together)                                         |
| **Channels WebSocket** (live editor, runner WS in legacy v3) | `apps/api/pi_dash/runner/consumers.py`, `apps/live/`             | Yes for runner — bumped on incompatible shape changes                  |
| **Runner ↔ cloud protocol**                                  | `runner/src/cloud/protocol.rs`, `apps/api/pi_dash/runner/views/` | **Yes** — wire version pinned, see [08](./08-cloud-runner-protocol.md) |

Internal REST is allowed to change freely because the web frontend and the API ship together. The runner protocol is the one external contract — it gates auto-update behavior. Bump deliberately.

## Where state lives

- **Postgres** — primary OLTP store (work items, users, runs, approvals, etc.). Django ORM models in `apps/api/pi_dash/db/` and per-app `models.py`.
- **Redis / Valkey** — Django Channels layer, cache, throttling, ephemeral pub/sub between web ↔ runner.
- **RabbitMQ** — Celery broker for background tasks (`apps/api/pi_dash/bgtasks/`).
- **MinIO / S3** — file/attachment object storage.
- **Local runner state** — TOML config + JSONL transcripts under the platform config/data dirs (`~/.config/pidash/...` on Linux). All on-disk secrets and the IPC socket are `0600`.

## What this wiki does **not** cover

- API endpoint reference — see DRF Spectacular at `/api/schema/swagger-ui/` when running locally.
- Frontend component catalog — see Storybook (`pnpm --filter @pi-dash/ui storybook`).
- The closed-source `private-pi-dash` cloud overlay — that lives in a separate repo and is documented there.
