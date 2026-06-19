# 05 — Frontend Architecture

The browser side of Pi Dash is **four React Router 7 apps** sharing a common workspace of TypeScript packages. State is managed with **MobX** (not Redux, not Zustand). Styling is Tailwind + the in-house `@pi-dash/ui` component library.

## The four apps

| App     | Folder        | Port       | Role                                                                                                                           |
| ------- | ------------- | ---------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `web`   | `apps/web/`   | 3000       | The main product UI — work items, cycles, modules, views, pages, analytics                                                     |
| `admin` | `apps/admin/` | 3001       | "God mode" instance admin (mounted at `/god-mode/`)                                                                            |
| `space` | `apps/space/` | 3002       | Public-facing "spaces" — published views accessible without login                                                              |
| `live`  | `apps/live/`  | per `.env` | **Backend service**, not a React app — Hocuspocus/Yjs realtime collaboration server (see [10](./10-realtime-collaboration.md)) |

`web`, `admin`, and `space` are structured similarly: `app/` for routes, `components/` for app-local components, `store/` for MobX root composition, `helpers/`, `hooks/`, `lib/`, plus `vite.config.ts`, `react-router.config.ts`, `Dockerfile.*`, `nginx/` (for prod static serving).

`apps/web/ce/` is the **Community Edition split**: code that's compiled-in for OSS builds but can be overridden in the proprietary build of Pi Dash Cloud (`private-pi-dash`). Treat `ce/` as the OSS implementation; do not fork it into private code.

## Shared packages (`packages/`)

```
ui/              ← component library + Storybook (pnpm --filter @pi-dash/ui storybook → :6006)
shared-state/    ← MobX stores: user, workspace, filters — the canonical client state
services/        ← API client (fetch wrappers, typed endpoints)
editor/          ← Tiptap/ProseMirror — collab editor (used with live server)
i18n/            ← translation runtime + English locale (OSS is English-only)
constants/       ← cross-app enums and string keys
hooks/           ← reusable React hooks
types/           ← cross-app TypeScript types
utils/           ← pure helpers
logger/          ← structured client logging
propel/          ← analytics / event tracking shim
decorators/      ← MobX/route decorators
tailwind-config/ ← shared Tailwind preset
typescript-config/ ← shared tsconfig bases
codemods/        ← jscodeshift transforms for repo-wide refactors
```

Apps consume packages via `"@pi-dash/<name>": "workspace:*"`. **External** deps go through the `catalog:` block in `pnpm-workspace.yaml` — see [03 — Repository layout](./03-repository-layout.md).

## State: MobX shared-state

The single source of truth for client state is `packages/shared-state`. Top-level stores include:

- `user.store.ts` — current user, session, preferences
- `workspace.store.ts` — current workspace, members, settings
- `work-item-filters/`, `rich-filters/` — UI filter state

Each app composes these stores in its own `store/` directory (a thin root store that wires the shared stores to app-specific stores). Components consume them via MobX observer / `useContext` — not React Context for the data itself, only for the root store handle.

**Convention:** any state that isn't strictly local to one component lives in a store, not in `useState`. Reactive patterns over prop drilling.

## Routing

Each app uses **React Router v7** (file-based + programmatic).

- Route files live under `apps/<app>/app/routes/`.
- Layout, root provider, error boundary in `apps/<app>/app/{root.tsx,layout.tsx,provider.tsx,error/}`.
- `routes.ts` and `routes/core.ts`/`extended.ts` define the route tree.

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

**Per-app `--max-warnings` ceilings** (pinned in each `package.json`):

| App           | Ceiling |
| ------------- | ------- |
| `web`         | 11957   |
| `space`       | 676     |
| `admin`       | 759     |
| `live`        | 119     |
| `@pi-dash/ui` | 66      |

Crossing the ceiling fails `pnpm check:lint`. After cleanups, **lower** the ceiling instead of leaving headroom. See [13 — Development workflow](./13-development-workflow.md) for the full toolchain.

## i18n

UI code uses source English text directly as the message id, for example `t("Create project")` — there's no separate key catalogue to maintain. The OSS build is English-only; the English locale lives in `packages/i18n/src/locales/en/`. Self-hosters can add their own languages — see `packages/i18n/README.md`. (Upstream multi-language locales and their sync/translate tooling are part of Pi Dash Cloud, not this repo.)

## Where to read next

- [06 — Backend architecture](./06-backend-architecture.md) — the REST/WS surface the frontend consumes
- [10 — Realtime collaboration](./10-realtime-collaboration.md) — `apps/live` and the editor
- `apps/web/app/routes/core.ts` for the actual route tree
