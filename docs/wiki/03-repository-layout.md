# 03 — Repository Layout

Pi Dash is a polyglot monorepo with three independent toolchains coexisting at the top level. Knowing which "stack" a directory belongs to is the fastest way to decide which commands to reach for.

```
pi-dash/
├── apps/
│   ├── web_new/         ← current product UI (TanStack Router)               :3010
│   ├── live/            ← Node + Hocuspocus (Yjs realtime)                   :3100
│   ├── proxy/           ← Caddy reverse proxy (production only)
│   └── web/ admin/ space/ api/  ← FROZEN fallback: legacy UIs + Django
│                                  backend (no new code, no behaviour change)
│
├── packages/            ← shared TS workspace packages
│   ├── kit/             ← component library for web_new (+ desktop target)
│   ├── api-client/      ← typed Rust-API client for web_new
│   ├── editor/          ← Tiptap / ProseMirror integration
│   ├── i18n/            ← locales + key sync tools
│   ├── constants/  hooks/  types/  utils/  logger/  propel/
│   ├── codemods/        ← jscodeshift transforms for repo-wide refactors
│   ├── tailwind-config/   typescript-config/    ← shared base configs
│   └── ui/ shared-state/ services/ decorators/  ← FROZEN: legacy packages
│                                                  consumed by apps/web|admin|space
│
├── rust-api/            ← current backend: axum routers + services + jobs
│   ├── crates/{types,db,auth,services,api,jobs,storage,sdk}
│   ├── bin/pidash-api/  ← `serve` (HTTP :8000) + `worker` (job queue)
│   ├── contract-tests/  ← pytest suites (Rust binary + Django oracle)
│   └── Dockerfile.api + bin/docker-entrypoint-*.sh
│
├── runner/              ← Rust binary crate (the `pidash` CLI + daemon)
│   ├── src/{cli,daemon,cloud,codex,approval,workspace,ipc,history,service,tui,config,util}
│   ├── install.sh / install.ps1   ← wrapper installers (auto-auth)
│   └── wix/             ← Windows MSI metadata
│
├── deployments/         ← reference deploys
│   ├── aio/community/   ← single-container all-in-one
│   ├── cli/community/   ← docker-compose self-host
│   ├── swarm/           ← Docker Swarm
│   └── kubernetes/community/  ← k8s manifests (Helm chart planned)
│
├── docs/                ← docs (you are here)
│   ├── wiki/            ← internal wiki — architecture / contributor guide
│   └── linting.md
│
├── images/              ← assets used in README / marketing
├── docker-compose.yml          ← self-host stack (Rust API + web_new + live)
├── docker-compose-local.yml    ← local dev: infra + migrator; apps run on host
├── turbo.json                  ← Turborepo task graph
├── pnpm-workspace.yaml         ← workspace + dep catalog
├── dist-workspace.toml         ← cargo-dist (runner release packaging)
├── setup.sh                    ← first-time env bootstrap
├── CLAUDE.md / AGENTS.md       ← in-repo guidance for coding agents
└── ...                         ← LICENSE / SECURITY / RELEASING / CODEOWNERS
```

## The three toolchains

| Stack                                 | Manifest                                                             | Manager          | Lockfile                        |
| ------------------------------------- | -------------------------------------------------------------------- | ---------------- | ------------------------------- |
| TS workspace (web_new, live, kit, api-client) | `package.json`, `pnpm-workspace.yaml`, `turbo.json` | pnpm + Turborepo | `pnpm-lock.yaml`  |
| Rust backend (`rust-api/`, own workspace)        | `rust-api/Cargo.toml`                               | cargo            | `rust-api/Cargo.lock` |
| Rust runner + ipc (root workspace)               | `runner/`, `pidash-ipc/`, `Cargo.toml`, `rust-toolchain.toml` | cargo | `Cargo.lock` |

**Important:** `pnpm-workspace.yaml` explicitly excludes `apps/api` (frozen Django) and `apps/proxy` (Caddy). They live inside `apps/` for proximity, but they are **not** in the pnpm workspace — root `pnpm` commands skip them. Never add the frozen paths back.

The Rust crates are similarly outside Turborepo. `cargo build` / `cargo test` is the way to build them — see `rust-api/README.md` and `runner/README.md`.

## Dependency conventions (TS workspace)

From `AGENTS.md`:

- **Internal packages** use `"workspace:*"` — e.g. `"@pi-dash/ui": "workspace:*"`.
- **External deps** use `"catalog:"` — versions are pinned in the `catalog:` block of `pnpm-workspace.yaml`. Don't add a direct version for anything that's already in the catalog.

When you upgrade a catalog dep, you upgrade it for every package in the workspace at once. That's the point.

## App vs package — when to put code where

- **An app (`apps/*`)** owns routes, top-level providers, and user-facing pages. Keep app folders thin — push reusable logic out.
- **A package (`packages/*`)** is everything reusable across two or more apps. Stores, API client, components, hooks, types, constants.

New code starts in an app and graduates to a package the second time it's needed somewhere else. Don't pre-create packages.

## Where the wiki points

- Want to change a UI route? → `apps/web_new/` + [05](./05-frontend-architecture.md).
- Want to add an API endpoint? → `rust-api/crates/api/` + [06](./06-backend-architecture.md).
- Want to teach the runner a new agent? → `runner/src/agent/` + `runner/src/codex/` + [07](./07-runner-architecture.md).
- Want to ship the runner? → [15 — Releasing](./15-releasing.md).
