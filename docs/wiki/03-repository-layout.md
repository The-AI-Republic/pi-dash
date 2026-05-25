# 03 — Repository Layout

Pi Dash is a polyglot monorepo with three independent toolchains coexisting at the top level. Knowing which "stack" a directory belongs to is the fastest way to decide which commands to reach for.

```
pi-dash/
├── apps/                ← TS/React frontends + Django backend + Node live server
│   ├── web/             ← main product UI            (React Router 7)         :3000
│   ├── admin/           ← instance-admin UI (/god-mode/)                       :3001
│   ├── space/           ← public "spaces" (guest views)                        :3002
│   ├── live/            ← Node/Express + Hocuspocus (Yjs realtime)
│   ├── api/             ← Django 4 / DRF / Channels backend (Python, separate)
│   └── proxy/           ← Caddy reverse proxy (production only)
│
├── packages/            ← shared TS workspace packages (consumed by apps/web|admin|space)
│   ├── ui/              ← component library + Storybook
│   ├── shared-state/    ← MobX stores (canonical client state)
│   ├── services/        ← API client
│   ├── editor/          ← Tiptap / ProseMirror integration
│   ├── i18n/            ← locales + key sync tools
│   ├── constants/  hooks/  types/  utils/  logger/  propel/  decorators/
│   ├── codemods/        ← jscodeshift transforms for repo-wide refactors
│   ├── tailwind-config/   typescript-config/    ← shared base configs
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
├── docker-compose.yml          ← production-flavored
├── docker-compose-local.yml    ← infra-only stack for `pnpm dev`
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
| TS/React frontends + Node live server | `package.json`, `pnpm-workspace.yaml`, `turbo.json`                  | pnpm + Turborepo | `pnpm-lock.yaml`                |
| Django backend                        | `apps/api/pyproject.toml`, `apps/api/requirements.txt`               | pip / virtualenv | (none — pinned in requirements) |
| Rust runner                           | `runner/Cargo.toml`, `Cargo.toml` (workspace), `rust-toolchain.toml` | cargo            | `Cargo.lock`                    |

**Important:** `pnpm-workspace.yaml` explicitly excludes `apps/api` (Django) and `apps/proxy` (Caddy). They live inside `apps/` for proximity, but they are **not** in the pnpm workspace — root `pnpm` commands skip them. Treat them as their own projects with their own commands.

The `runner/` crate is similarly outside Turborepo. `cargo build` is the only way to build it.

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

- Want to change a UI route? → `apps/web/app/routes/` + [05](./05-frontend-architecture.md).
- Want to add an API endpoint? → `apps/api/pi_dash/{app,api}/views/` + [06](./06-backend-architecture.md).
- Want to teach the runner a new agent? → `runner/src/agent/` + `runner/src/codex/` + [07](./07-runner-architecture.md).
- Want to ship the runner? → [15 — Releasing](./15-releasing.md).
