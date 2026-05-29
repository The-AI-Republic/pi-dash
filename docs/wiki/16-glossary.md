# 16 — Glossary

Terms you'll see across the wiki, code, UI, and tickets. Where a term has a Pi-Dash-specific meaning, it's noted.

## Product

**Workspace**
The top-level container a user logs into. Holds projects, members, settings. A user can belong to many workspaces.

**Project**
A discrete unit of work inside a workspace — a codebase, a team's board, etc. Owns work items, cycles, modules, members. A project can be connected to runners.

**Work item** (a.k.a. _issue_, _task_)
The user-facing unit of tracked work. Has a title, description, state, assignee, labels, parent/sub-issue links, etc. Lives in `apps/api/pi_dash/app/`.

**Cycle**
A time-boxed iteration (sprint). Work items can be assigned to a cycle. Has start/end dates, progress analytics.

**Module**
A grouping of work items by theme (epic / feature area). Cuts across cycles.

**View**
A saved filter / sort / display configuration over work items.

**Page**
A rich-text document inside a project (docs, RFCs, retros). Real-time collaboratively editable via `apps/live` — see [10](./10-realtime-collaboration.md).

**Space**
A _public_ view of project content. Served by `apps/space/`. Accessible without login when published.

**Instance admin** ("god mode")
The first user registered against a self-hosted instance. Lives in `apps/admin/` at `/god-mode/`. Has cross-workspace privileges. See [11](./11-authentication.md).

## Agent execution

**Runner**
A registered instance of the `pidash` daemon on a developer machine, scoped to a project. One host can run multiple runners (one per project). Model in `pi_dash/runner/models.py`.

**Pod**
A grouping primitive on the runner side — a namespace a run lives in on a particular runner. Naming logic in `pi_dash/runner/services/pod_naming.py`.

**Run**
One execution attempt of an agent against a work item. Persisted server-side; transcripts persisted locally on the runner under its data dir. A work item can have many runs (retries, follow-ups).

**Phase**
A step within a run. Runs progress through ordered phases (defined in `pi_dash/orchestration/agent_phases.py`); each phase has its own prompt template and expected outputs.

**Workpad**
The durable scratchpad attached to a run that the agent reads/writes structured artifacts into. Defined in `pi_dash/orchestration/workpad.py`.

**Done signal**
The explicit completion contract the agent emits to mark a phase done. Parsed by `pi_dash/orchestration/done_signal.py`.

**Approval**
A user decision the agent is waiting on (run command, write file, fetch URL). Decision sources race; **first writer wins** (TUI vs cloud vs local policy). Router in `runner/src/approval/`.

**Matcher**
Server-side logic that assigns an eligible run to an available runner. `pi_dash/runner/services/matcher.py`.

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
The user-identifying token minted by `pidash auth login` (device-code flow). Stored at `~/.config/pidash/config.toml` (`0600`). Authorizes `pidash runner add`.

**Refresh token / access token (per runner)**
Token pair held by each registered runner. Refresh is long-lived and rotatable; access is short-lived and used per request. Logic in `pi_dash/runner/services/tokens.py`.

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
A pinned OxLint warning count per app (e.g. `web: 11957`). `pnpm check:lint` fails if warnings exceed the ceiling. After cleanup, **lower** the ceiling; never raise it as a workaround.

**God mode**
Casual name for the instance admin UI (`apps/admin`, mounted at `/god-mode/`).

**Pi Dash CE**
Community Edition — the OSS variant. Code under `apps/web/ce/` is the OSS implementation that can be overridden in the Cloud build.

**`private-pi-dash`**
The closed-source repo that overlays Pi Dash Cloud features on top of this OSS repo. Don't fork OSS files into it.

## Where to read next

- [01 — Architecture overview](./01-architecture-overview.md) — how the moving parts above fit together
- [09 — Agent orchestration](./09-agent-orchestration.md) — the run → phase → workpad → done-signal pipeline in depth
