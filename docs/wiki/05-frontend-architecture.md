# 05 — Frontend Architecture

The browser side of Pi Dash is the new React frontend in **`apps/web_new/`**, built on **`packages/kit/`** (component library) and **`packages/api-client/`** (typed client for the Rust API). It uses **TanStack Router** for routing and **React Query** for server state, targets both web and desktop (Tauri) builds, and talks to `apps/live/` for realtime collaboration. Styling is Tailwind.

## The current apps

| App       | Folder          | Port              | Role                                                                                                  |
| --------- | --------------- | ----------------- | ----------------------------------------------------------------------------------------------------- |
| `web_new` | `apps/web_new/` | 3010 (dev)        | The product UI — work items, cycles, modules, views, pages, analytics, incl. `/god-mode/` admin      |
| `live`    | `apps/live/`    | 3100 (per `.env`) | **Backend service**, not a React app — Hocuspocus/Yjs realtime collaboration server (see [10](./10-realtime-collaboration.md)) |

`web_new` ships two targets from one codebase: `build:web` (browser, prerendered) and `build:desktop` (Tauri). Coexistence with the legacy UI is generated from `apps/web_new/migrated-routes.json` into the Caddy config (`coexistence:generate` / `check:coexistence`) — see [12 — Deployment](./12-deployment.md).

## Frozen legacy frontends

`apps/web/`, `apps/admin/`, and `apps/space/` (React Router 7 + MobX + `@pi-dash/ui`) are **frozen and kept only as a fallback**: no bug fixes, no new features. Behavioural parity scenarios under `apps/web_new/e2e/parity/` encode the old UI's behaviour as the reference; scenarios titled `bug:` mark old-app bugs the new app deliberately does not copy. Never import from or copy code from the legacy apps into `web_new`, `kit` or `api-client` — read them to learn behaviour, then write new code (the `check:boundaries`, `check:headers`, `check:deps` and `check-similarity` gates enforce this).

## Shared packages (`packages/`)

```
kit/             ← component library for web_new (unit-tested, web + desktop targets)
api-client/      ← typed client for the Rust API (fetch wrappers, typed endpoints)
editor/          ← Tiptap/ProseMirror — collab editor (used with live server)
i18n/            ← translation runtime + English locale (OSS is English-only)
constants/       ← cross-app enums and string keys
hooks/           ← reusable React hooks
types/           ← cross-app TypeScript types
utils/           ← pure helpers
logger/          ← structured client logging
propel/          ← analytics / event tracking shim
tailwind-config/ ← shared Tailwind preset
typescript-config/ ← shared tsconfig bases
codemods/        ← jscodeshift transforms for repo-wide refactors
```

`ui/`, `shared-state/`, `services/` and `decorators/` are the frozen legacy packages consumed by `apps/web|admin|space` — do not import them into `web_new` (see `check:boundaries`).

Apps consume packages via `"@pidash/<name>": "workspace:*"`. **External** deps go through the `catalog:` block in `pnpm-workspace.yaml` — see [03 — Repository layout](./03-repository-layout.md).

## State: server state + local state

Server state in `web_new` lives in **React Query** (caching, invalidation, background refetch around `packages/api-client/` calls). Client state lives in **zustand** stores plus component state. (The legacy MobX `packages/shared-state` belongs to the frozen apps — do not use it in `web_new`.)

**Convention:** any state that isn't strictly local to one component lives in a query or a store, not in prop drilling.

## Routing

`web_new` uses **TanStack Router** (type-safe, code-based route tree).

- Routes live under `apps/web_new/` and are typechecked end to end (`typecheck` script).
- The desktop target reuses the same tree with Tauri bindings (`check:no-tauri` keeps web-only imports out of shared code).

## Build & dev tooling

- **Vite** for dev server + bundling (`vite.config.ts` in each app).
- **`react-router.config.ts`** for RR7-specific config (SSR mode, route discovery).
- **Turborepo** drives `dev` / `build` / `check` across the workspace. `pnpm dev` runs `turbo run dev --concurrency=18`.
- **`packages/*` build** with `tsdown` (Rust-based, fast).
- **Storybook** is run inside `@pi-dash/ui` only — `pnpm --filter @pi-dash/ui storybook` → `:6006`. Build components there in isolation before wiring them into an app.

## Linting & formatting

The entire TS workspace shares **one** root config:

- `.oxlintrc.json` — OxLint rules
- `.oxfmtrc.json` — oxfmt formatter

Both tools are Rust-based, replacing ESLint + Prettier. `eslint-disable` comments still work for back-compat.

**`web_new` gates** (all must pass; never weaken or skip them):

| Check                 | What it guards                                              |
| --------------------- | ----------------------------------------------------------- |
| `lint` (`--deny-warnings`) | Zero-warning OxLint                                    |
| `check:boundaries`    | No imports from the frozen legacy frontend into the new one |
| `check:headers`       | License headers                                             |
| `check:deps`          | Dependency hygiene                                          |
| `check:similarity`    | No ported/translated legacy code (`check-similarity.mjs`)   |
| `typecheck`           | Strict `tsc --noEmit`                                       |

See [13 — Development workflow](./13-development-workflow.md) for the full toolchain.

## i18n

UI code uses source English text directly as the message id, for example `t("Create project")` — there's no separate key catalogue to maintain. The OSS build is English-only; the English locale lives in `packages/i18n/src/locales/en/`. Self-hosters can add their own languages — see `packages/i18n/README.md`. (Upstream multi-language locales and their sync/translate tooling are part of Pi Dash Cloud, not this repo.)

## Where to read next

- [06 — Backend architecture](./06-backend-architecture.md) — the Rust REST surface the frontend consumes
- [10 — Realtime collaboration](./10-realtime-collaboration.md) — `apps/live` and the editor
- `apps/web_new/e2e/parity/` for the behavioural parity scenarios against the legacy UI
