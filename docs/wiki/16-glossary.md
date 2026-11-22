# 16 — Glossary

Terms you'll see across the wiki, code, UI, and tickets. Where a term has a Pi-Dash-specific meaning, it's noted.

## Product

**Workspace**
The top-level container a user logs into. Holds projects, members, settings. A user can belong to many workspaces.

**Project**
A discrete unit of work inside a workspace — a codebase, a team's board, etc. Owns work items, cycles, modules, members. A project can be connected to runners.

**Work item** (a.k.a. _issue_, _task_)
The user-facing unit of tracked work. Has a title, description, state, assignee, labels, parent/sub-issue links, etc. Served by the `app_*` domains of the Rust backend.

**Cycle**
A time-boxed iteration (sprint). Work items can be assigned to a cycle. Has start/end dates, progress analytics.

**Module**
A grouping of work items by theme (epic / feature area). Cuts across cycles.

**View**
A saved filter / sort / display configuration over work items.

**Page**
A rich-text document inside a project (docs, RFCs, retros). Real-time collaboratively editable via `apps/live` — see [10](./10-realtime-collaboration.md).

**Space**
A _public_ view of project content. Served by the `space` domain of the Rust backend (the legacy `apps/space/` UI is frozen). Accessible without login when published.

**Instance admin** ("god mode")
The first user registered against a self-hosted instance. Lives in `apps/web_new/` at `/god-mode/`. Has cross-workspace privileges. See [11](./11-authentication.md).

## Agent execution

**Runner**
A registered instance of the `pidash` daemon on a developer machine, scoped to a project. One host can run multiple runners (one per project). Persisted server-side in the runner tables managed by the Rust backend.

**Pod**
A grouping primitive on the runner side — a namespace a run lives in on a particular runner.

**Run**
One execution attempt of an agent against a work item. Persisted server-side; transcripts persisted locally on the runner under its data dir. A work item can have many runs (retries, follow-ups).

**Phase**
A step within a run. Runs progress through ordered phases; each phase has its own prompt template and expected outputs.

**Workpad**
The durable scratchpad attached to a run that the agent reads/writes structured artifacts into.

**Done signal**
The explicit completion contract the agent emits to mark a phase done.

**Approval**
A user decision the agent is waiting on (run command, write file, fetch URL). Decision sources race; **first writer wins** (TUI vs cloud vs local policy). Router in `runner/src/approval/`.

**Matcher**
Server-side logic that assigns an eligible run to an available runner. Lives in the Rust orchestration services.

## Protocol / wire

**Wire version**
Integer identifying the runner ↔ cloud protocol shape. Currently `4`. Bumped only on incompatible changes. Source: `runner/src/cloud/protocol.rs`.

**Welcome frame**
The first response the cloud returns when a runner opens a session. Includes the accepted `protocol_version` plus optional `latest_runner_version` and `min_runner_version` advisories. See [08](./08-cloud-runner-protocol.md).

**Long-poll**
The v4 transport — runner holds an HTTP request open until the cloud has work or the timeout fires. Replaced the v1–v3 persistent WebSocket.

**Auto-update**
The runner's ability to swap its on-disk binary in place when the cloud advertises a newer `latest_runner_version`. Running process is **never** disturbed; the new binary takes effect on the next natural restart.

## Auth

**CLI token**
The dev-machine `MachineToken` minted by `pidash auth login` (device-code flow). Stored at `~/.config/pidash/config.toml` (`0600`). Authorizes CLI commands and every runner on that dev machine.

**MachineToken**
One shared `mt_...` credential per dev machine/workspace. Runner runtime requests pair this token with an explicit runner id, so runners remain separate management units without separate secrets.

**Device-code flow**
Browser-based login that requires no token paste. Same UX as `gh auth login` / `stripe login`. Recommended.

**Enrollment token** (deprecated)
One-shot token used by the hidden `pidash connect` compatibility path. New runner setup uses `pidash auth login` and `pidash runner add`.

**Plan**
The current subscription tier of an account on Pi Dash Cloud. Source-of-truth is the home-page OIDC; cached as a JWT `plan` claim on `Account`. No webhooks. Upgrade UI deep-links back to home-page.

## Tooling / repo

**Catalog dep**
An external dep version pinned in the `catalog:` block of `pnpm-workspace.yaml`. Apps and packages reference these via `"catalog:"` rather than a literal version. See [03](./03-repository-layout.md) and `AGENTS.md`.

**Max-warnings ceiling**
A pinned OxLint warning count per legacy app. `pnpm check:lint` fails if warnings exceed the ceiling — never raise one. (`web_new` lints with `--deny-warnings` instead.)

**God mode**
Casual name for the instance admin UI (`apps/web_new`, mounted at `/god-mode/`).

**Pi Dash CE**
Community Edition — the OSS variant of this repo, as opposed to the Pi Dash Cloud overlay in `private-pi-dash`. (The old per-app `ce/` split is frozen with the legacy frontends.)

**`private-pi-dash`**
The closed-source repo that overlays Pi Dash Cloud features on top of this OSS repo. Don't fork OSS files into it.

## Where to read next

- [01 — Architecture overview](./01-architecture-overview.md) — how the moving parts above fit together
- [09 — Agent orchestration](./09-agent-orchestration.md) — the run → phase → workpad → done-signal pipeline in depth
