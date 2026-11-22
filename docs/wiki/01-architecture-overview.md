# 01 — Architecture Overview

Pi Dash is an **AI agent orchestration platform** built for "As Coding" (asynchronous vibe coding): you define what needs to be built, agents implement it in the background, and you spend your time scoping and reviewing rather than watching terminals.

The product is the orchestration layer. It does **not** ship its own coding agent — you bring `claude` or `codex`, and Pi Dash drives them.

## The three components

```
┌───────────────────────────┐         ┌────────────────────────────┐
│   Pi Dash Platform        │         │    Pi Dash Runner          │
│   (cloud / self-hosted)   │  ◀─────▶│    (on your dev machine)   │
│                           │  HTTPS  │                            │
│ • Web UI (`apps/web_new`) │  + WS   │ • Rust daemon (`pidash`)   │
│ • Rust API (`rust-api`)   │         │ • Supervisor + state mach. │
│ • Realtime (`apps/live`)  │         │ • Codex / Claude bridge    │
│ • Postgres job queue      │         │ • Local TUI + IPC socket   │
│   (Rust worker)           │         │                            │
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

The orchestration hub: work items, cycles, modules, views, pages, analytics — plus the cloud side of the runner protocol. It is the Rust API in `rust-api/` (binary `pidash-api`: `serve` for HTTP, `worker` for background jobs) with the new React frontend in `apps/web_new/` and the realtime server in `apps/live/`, behind the Caddy reverse proxy in `apps/proxy/`.

The Django backend (`apps/api/`) and the legacy frontends (`apps/web/`, `apps/admin/`, `apps/space/`) are frozen and kept only as a fallback — see [03 — Repository layout](./03-repository-layout.md).

See [05 — Frontend architecture](./05-frontend-architecture.md) and [06 — Backend architecture](./06-backend-architecture.md).

### 2. Pi Dash Runner

A single Rust binary (`pidash`) installed on a developer machine. It authenticates against the platform, polls for assigned tasks, dispatches each one to the user's configured AI agent as a subprocess, and reports results back. Lives under `runner/`.

See [07 — Runner architecture](./07-runner-architecture.md) and [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md).

### 3. AI Agent (user-provided)

Pi Dash is agent-agnostic. The runner ships first-class support for **Claude Code** (`claude`) and **Codex** (`codex`); the dispatch layer is designed so other agents can be wired in without changing the orchestration model. Users install the agent binary themselves; `pidash doctor` checks it.

## End-to-end task lifecycle

1. **User creates a work item** in the web UI (`apps/web_new`).
2. **Platform assigns** the work item to a project that has at least one connected runner.
3. **Orchestration layer** (the Rust services in `rust-api/crates/services/`, driven by the `worker`) produces an agent run plan — phase, prompt, working dir, approval policy — and persists it.
4. **Runner polls** the cloud for new runs over HTTPS long-poll (wire protocol v4, see [08](./08-cloud-runner-protocol.md)).
5. **Runner spawns** the configured agent (`codex app-server` or `claude`) in the project's workspace, streaming the JSON-RPC bridge into the daemon.
6. **Approval router** (first-writer-wins between TUI / cloud / policy) handles any approval prompts the agent emits.
7. **Run transcripts** (JSONL) are written locally under the runner's data dir, and progress events are pushed back to the platform.
8. **User reviews** results in the web UI — diff, transcript, approval decisions, follow-up.

## The three seams to know

When a change spans more than one layer, you almost always cross one of these:

| Seam                                                         | Files                                                            | Versioned?                                                             |
| ------------------------------------------------------------ | ---------------------------------------------------------------- | ---------------------------------------------------------------------- |
| **REST API** (browser ↔ Rust API)                          | `rust-api/crates/api/` routers + `packages/api-client/`          | No (internal, deploy together)                                         |
| **Realtime WebSocket** (live editor)                         | `apps/live/` (Yjs/Hocuspocus), session validated by the Rust API | No (internal, deploy together)                                         |
| **Runner ↔ cloud protocol**                                  | `runner/src/cloud/protocol.rs`, `rust-api/` runner handlers      | **Yes** — wire version pinned, see [08](./08-cloud-runner-protocol.md) |

Internal REST is allowed to change freely because the web frontend and the API ship together. The runner protocol is the one external contract — it gates auto-update behavior. Bump deliberately.

## Where state lives

- **Postgres** — primary OLTP store (work items, users, runs, approvals, etc.). The Rust backend reads it through SeaORM entities in `rust-api/crates/db/src/entity/` (Django's migration history in `apps/api/` stays the single schema history for the shared database, so the frozen fallback can still run against it).
- **Postgres job queue** — background jobs run on the `rust_job_queue` tables drained by `pidash-api worker`. There is no AMQP path in the active system (the old Celery + RabbitMQ broker is frozen with the Django backend).
- **Redis / Valkey** — cache, throttling, ephemeral pub/sub between web ↔ runner.
- **MinIO / S3** — file/attachment object storage.
- **Local runner state** — TOML config + JSONL transcripts under the platform config/data dirs (`~/.config/pidash/...` on Linux). All on-disk secrets and the IPC socket are `0600`.

## What this wiki does **not** cover

- API endpoint reference — see the contract suites in `rust-api/contract-tests/` (run against the Rust binary; the Django server is the oracle for the old system's behaviour).
- Frontend component catalog — see the unit-tested components in `packages/kit/` (the `@pi-dash/ui` Storybook is the frozen legacy path).
- The closed-source `private-pi-dash` cloud overlay — that lives in a separate repo and is documented there.
