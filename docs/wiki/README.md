# Pi Dash Wiki

Internal reference docs for contributors and operators of Pi Dash — the AI agent orchestration platform.

These pages explain how the system fits together. They complement (not replace) the top-level `README.md` (user-facing install + product pitch) and `CLAUDE.md` (terse repo cheatsheet for coding agents).

> **New to Pi Dash as a user?** Jump straight to **[02 — Pi Dash Cloud Quickstart](./02-cloud-quickstart.md)** — a 15-minute, task-oriented walkthrough from "I just signed up" to "my first agent run shipped a diff." The rest of this wiki is for contributors who want to understand or change the codebase.

## Start here

| Doc                                                         | What it covers                                                                 |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------ |
| [01 — Architecture overview](./01-architecture-overview.md) | The three Pi Dash components, how a task flows end-to-end, where the seams are |
| [02 — Pi Dash Cloud Quickstart](./02-cloud-quickstart.md)   | User-facing: sign up → install CLI → connect runner → first agent run          |
| [03 — Repository layout](./03-repository-layout.md)         | Polyglot monorepo tour: `apps/`, `packages/`, `rust-api/`, `runner/`, `deployments/` |
| [04 — Getting started locally](./04-getting-started.md)     | First-time `setup.sh`, `docker compose`, cargo `serve`/`worker`, key URLs, god-mode |

## Per-stack deep dives

| Doc                                                           | What it covers                                                                        |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| [05 — Frontend architecture](./05-frontend-architecture.md)   | `web_new` + `kit`/`api-client`, TanStack Router, React Query, frozen legacy apps      |
| [06 — Backend architecture](./06-backend-architecture.md)     | Rust API crates, serve/worker, edge fallback, contract tests, schema                  |
| [07 — Runner architecture](./07-runner-architecture.md)       | Rust crate layout, daemon supervisor, Codex bridge, IPC, TUI                          |
| [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md) | Wire version, welcome frame, auto-update, version pinning                             |
| [09 — Agent orchestration](./09-agent-orchestration.md)       | Rust `orchestration` + `prompting` + `scheduler`: how a work item becomes an agent run |
| [10 — Realtime collaboration](./10-realtime-collaboration.md) | `apps/live` Yjs/Hocuspocus server and the Tiptap editor                               |
| [11 — Authentication](./11-authentication.md)                 | Instance admin, OIDC, device-code login, runner enrollment & token rotation           |

## Operating Pi Dash

| Doc                                                       | What it covers                                                           |
| --------------------------------------------------------- | ------------------------------------------------------------------------ |
| [12 — Deployment topologies](./12-deployment.md)          | AIO container vs Compose/Swarm vs Kubernetes                             |
| [13 — Development workflow](./13-development-workflow.md) | pnpm/turbo/cargo, OxLint + oxfmt, `web_new` gates, Husky, catalog deps   |
| [14 — Testing](./14-testing.md)                           | Contract suites, vitest, cargo test, quality gates                       |
| [15 — Releasing](./15-releasing.md)                       | Runner cargo-dist tags, platform release flow, env-var version announcements |
| [16 — Glossary](./16-glossary.md)                         | Workspace, project, work item, cycle, module, runner, pod, run          |
| [17 — `pidash` CLI reference](./17-cli-reference.md)      | Complete reference for every `pidash` subcommand and flag               |

## How to contribute to the wiki

- Edit existing pages in place rather than adding new ones unless a topic needs its own home.
- Keep each page focused on a single subsystem or workflow. Cross-link aggressively.
- Code-level docstrings and `CLAUDE.md`/`AGENTS.md` are still the source of truth for day-to-day commands. The wiki is for _why_ and _how it fits together_.
- When you rename a directory or rip out a module, grep this wiki and update the references.
