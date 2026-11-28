# Agent Development Guide

## Repository shape

This repo is a polyglot monorepo. The active system is the Rust API plus the new frontend; the Django backend and the legacy frontends are frozen and kept only as a fallback (see "Frozen fallback" below):

- **Rust backend:** `rust-api/` — the current API server. Workspace of crates under `rust-api/crates/` (`compat → types → {auth, db} → storage → services → jobs → {api, ops} → sdk`), plus the server binary in `rust-api/bin/pidash-api/` (`serve` on port 8000, `worker` for background jobs), contract tests in `rust-api/contract-tests/`, and fixtures in `rust-api/fixtures/`.
- **TypeScript (pnpm + Turbo):** `apps/web_new/` — the current frontend — plus `apps/live/` (realtime collab server) and `packages/*` (including `packages/kit/` and `packages/api-client/`). Managed by `pnpm-workspace.yaml`.
- **Daemon:** `runner/` — its own Cargo crate (package `pidash`, binary `pidash`). It connects developer machines to the cloud and drives issue work. Shared desktop/daemon wire types live in `pidash-ipc/`.
- **Desktop app:** `desktop/` (Tauri host in `desktop/src-tauri/`), pairing with `apps/web_new/` in desktop mode.

Frozen fallback (do not change, do not build on): the Django backend in `apps/api/` (Python, excluded from the pnpm workspace), the legacy frontends `apps/web/`, `apps/admin/`, `apps/space/`, the old `@pi-dash/*` packages (`packages/*` other than `kit`, `api-client` and shared config packages the new frontend already uses), and `desktop-overlay/`.
- **Desktop app:** `desktop/` (Tauri host in `desktop/src-tauri/`), pairing with `apps/web_new/` in desktop mode.

## Agent vocabulary

Pi Dash has three kinds of agent and one chat product. Use exactly these names. Full spec: `.ai_design/agent_naming_convention/design.md`.

- **Pi Dash Assistant** — the chat product: the UI plus the backend for threads, messages and turns. It has two engines, chosen by where it runs.
- **Cloud agent** (`cloud_agent`) — the multi-tenant, cloud-based agent engine: model routing, the model loop, provider calls, tool-call handling, usage limits. It only answers the assistant's chat in the cloud (web) version. It is not a runner and never executes an issue run.
- **Pi Dash Agent** (`pidash_agent` / `PidashAgent`) — the native agent built on the Codex source, installed along with the Pi Dash CLI daemon. It runs issue work, and it answers the assistant's chat in the desktop app.
- **Third-party agents** — agents the user brings (Claude Code, Codex, Cursor Agent, and so on), run through the daemon for issue work.

```
 Pi Dash Assistant (chat product)
      |-- cloud version --> cloud agent      (server-side engine)
      |-- desktop app ----> Pi Dash Agent    (local, through the daemon)

 Issue runs
      |-- Pi Dash Agent
      |-- third-party agents
      (never the cloud agent)
```

Rules for code:

1. `cloud_agent` means the chat engine and nothing else.
2. The dependency reads "assistant calls cloud agent": assistant code imports the engine; the engine imports nothing from the assistant.
3. The native agent has one name. "engine", `managed_runner`, "built-in agent", "built-in engine" and "bundled engine" become Pi Dash Agent in everything a developer or user reads.
4. Stored and wire values do not change: anything a database row, installed daemon, older desktop build, config file or the shared database (which the frozen Django fallback still reads) depends on keeps its exact spelling (design doc section 8). Only the Rust/TypeScript names around those values change.
5. Do not rename "engine" blindly: it also means "any agent process" and "the chat engine" in places (design doc section 6.4).

## Commands

- `pnpm dev` - Start all workspace dev servers, including the frozen legacy apps (active-system work uses the filtered commands in [04 — Getting started](./docs/wiki/04-getting-started.md): `web_new` on :3010, `live` on :3100, Rust API on :8000)
- `pnpm build` - Build all packages and apps
- `pnpm check` - Run all checks (format, lint, types)
- `pnpm check:lint` - OxLint across all packages
- `pnpm check:types` - TypeScript type checking
- `pnpm fix` - Auto-fix format and lint issues
- `pnpm turbo run <command> --filter=<package>` - Target specific package/app
- `pnpm --filter=@pi-dash/ui storybook` - Start Storybook on port 6006 (legacy `@pi-dash/ui` package, frozen fallback; `web_new` builds components in `packages/kit/` with unit tests instead)

## Code Style

- **Imports**: Use `workspace:*` for internal packages, `catalog:` for external deps
- **TypeScript**: Strict mode enabled, all files must be typed
- **Formatting**: oxfmt, run `pnpm fix:format`
- **Linting**: OxLint with shared `.oxlintrc.json` config
- **Naming**: camelCase for variables/functions, PascalCase for components/types
- **Error Handling**: Use try-catch with proper error types, log errors appropriately
- **State Management**: MobX stores in `packages/shared-state`, reactive patterns
- **Testing**: All features require unit tests, use existing test framework per package
- **Components**: Build in `packages/kit` with unit tests for isolated development (`@pi-dash/ui` + Storybook is the frozen legacy path)
