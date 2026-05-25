# 10 — Realtime Collaboration

Pi Dash supports realtime multi-user editing of rich-text content (pages, work item descriptions, comments). This is done by a dedicated **Node service**, not by Django Channels — Django's WS layer is reserved for the runner protocol and per-feature notifications.

## The two pieces

### 1. `apps/live` — Hocuspocus server (backend)

Located at `apps/live/`. This is a **Node + Express server** that hosts a [Hocuspocus](https://tiptap.dev/hocuspocus/introduction) collaboration backend on top of [Yjs](https://github.com/yjs/yjs) CRDTs.

```
apps/live/
├── src/                  ← server entry, persistence, auth hooks
├── tests/                ← vitest test suite
├── tsdown.config.ts      ← Rust-based bundler config
├── vitest.config.ts
├── Dockerfile.live
└── Dockerfile.dev
```

Runs with `node --env-file=.env`. Builds via `tsdown`. The default OxLint ceiling is **119** warnings.

**Responsibilities:**

- Accept WebSocket connections from editor clients (`@pi-dash/editor`).
- Sync Yjs documents between clients (CRDT merge — conflict-free).
- Authenticate each connection against the Pi Dash session.
- Persist document snapshots back to the main store (so reopening a page picks up where you left off).
- Broadcast presence (cursors, selections).

### 2. `@pi-dash/editor` — Tiptap/ProseMirror editor (frontend)

Located at `packages/editor/`. This is the React component layer that:

- Wraps Tiptap (which wraps ProseMirror) for the editing UI.
- Wraps the Hocuspocus client to talk to `apps/live`.
- Exposes typed editor instances to `apps/web`, `apps/space`, and (where applicable) `apps/admin`.

Pages, rich descriptions, and inline comment editors all consume this package.

## How it connects

```
Browser tab                       apps/live                    Postgres
┌─────────────────┐  WS (Yjs)  ┌──────────────┐   persist   ┌──────────┐
│ @pi-dash/editor │ ─────────▶ │ Hocuspocus   │ ──────────▶ │ docs/    │
│  (Tiptap)       │            │  + Express   │             │ pages    │
└─────────────────┘            └──────────────┘             └──────────┘
       ▲                              │
       │  WS (presence + ops)         │ Pi Dash session auth
       ▼                              ▼
┌─────────────────┐            ┌──────────────┐
│ Other tabs /    │            │ Pi Dash API  │  (apps/api)
│ collaborators   │            │ /auth/...    │
└─────────────────┘            └──────────────┘
```

- Clients **do not** talk to Django for live edits — only `apps/live`.
- `apps/live` validates the session with the Django API on connect.
- Persistence is async: ops flow over WS in real time; snapshots are flushed to Postgres at intervals / on disconnect.

## Why a separate Node service?

- Django Channels is single-process per worker and not ideal for the steady WS load of many open editors.
- Hocuspocus + Yjs are mature Node libraries — reimplementing in Python would buy nothing.
- Keeps the realtime concern out of the REST request hot path.

The trade-off: `apps/live` is **deployed alongside** the other web apps. In Docker, it's its own service container.

## Dev loop

`pnpm dev` (root) starts `apps/live` along with the other apps. Port is set per `.env` (`apps/live/.env`).

Test it in isolation:

```bash
pnpm --filter live dev
pnpm --filter live test
```

## Production

- Caddy (`apps/proxy/Caddyfile.*`) fronts `apps/live` and upgrades the WS connection.
- In `deployments/cli/community/` and Swarm/K8s, `apps/live` is a separate service alongside `web`/`admin`/`space`/`api`.
- In the AIO single-container build, `supervisord` keeps the Node process up next to Django/Caddy/Postgres.

## Where to read next

- [05 — Frontend architecture](./05-frontend-architecture.md) — how `@pi-dash/editor` plugs into apps
- [12 — Deployment](./12-deployment.md) — where `apps/live` sits in each topology
- `apps/live/src/` — the server entry, persistence hooks, auth bridge
