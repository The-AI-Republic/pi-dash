# Django backend inventory and Rust migration slices

Status: **draft for review** (PIDASHCONV-2). Analysis only — no code.
Public document: describes OSS extension points only, never private overlay internals.

**Source of truth for issue creation.** Conversion issues are filed directly from
this document: the foundation list in §6a and the domain-epic table in §6b are the filing
units, §9 is the filer's contract. There is no TSV middleman and no generation script: a
human or an agent run files each epic through the `pidash` CLI (or MCP) from the row, and
the epic's **first agent run splits it into layered sub-issues** after reading the code
(the platform's split-into-children behaviour, PDASHOSS01-169, with `blocked_by` edges,
PDASHOSS01-199). Where the general design doc names `GRAPH.tsv`, `MODELS.tsv`,
`ENDPOINTS.tsv`, a generation script or per-layer fixture generators, this document
supersedes it. The `Disposition` column (`full` | `minimal` | `skip`) is the stage-0
verdict; `skip` rows are never filed.

All numbers were measured at OSS `main` (`ddaef0c2`) with `grep`/line counts over
`apps/api/pi_dash`, excluding `migrations/` and the central `tests/` tree unless noted.
First measured at `ce3cf2ad`; re-measured 2026-09-23 after 100+ commits of drift
(concentrated in `prompting`, `api`, `assistant`, `runner`, `orchestration`,
`utils`, `db`, plus 8 new migrations).
Class counts use `^class …(…Serializer|…APIView|…ViewSet…)` line regexes, so they are
approximate the same way in every area; route counts count `path(`/`re_path(`/`router.register`
call sites plus DRF `@action`s. Per-area route/view/serializer counts below were
verified at the first measurement; `Size` (non-migration, non-test Python LOC) and
the global totals were re-measured at `ddaef0c2`; rows marked ↻ changed on re-measure.

URL-prefix map (from `apps/api/pi_dash/urls.py`, verified unchanged at `ddaef0c2`):

| Prefix           | Module                                          | Clients                                                   |
| ---------------- | ----------------------------------------------- | --------------------------------------------------------- |
| `api/`           | `app.urls` (22 modules)                         | web, admin (session auth)                                 |
| `api/`           | `assistant.urls`, `loop.urls`, `prompting.urls` | web (AI assistant, auto-pm, prompt admin)                 |
| `api/public/`    | `space.urls`                                    | space frontend (public deploy boards)                     |
| `api/instances/` | `license.urls`                                  | admin (instance console)                                  |
| `api/runners/`   | `runner.web_urls`                               | web (agent-run management, session auth)                  |
| `api/v1/`        | `api.urls` (16 modules)                         | external API tokens                                       |
| `api/v1/runner/` | `runner.urls`                                   | runner agents (machine tokens), desktop                   |
| `auth/`          | `authentication.urls`                           | web, admin, space (`spaces/…`), desktop/CLI (device flow) |
| `/`              | `web.urls` (`robots.txt`, health check)         | load balancers, crawlers                                  |
| `ws/runner/`     | `runner/routing.py`                             | retired — stub consumer rejects all traffic               |

## 1. Area-by-area inventory

Size = non-migration, non-test Python LOC in the area directory.

| Area              | What it is                                                                                                                                                                                                                                                                                                                                                                                             | Size                                                                                   | Routes / Views / Serializers / Models                                                                                                                                                                           | Tasks / Signals / Tests                                                                                                                    | Main clients                                                           | Coupling                                                                                                             | Risk                                                                                                                                                             |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app`             | Core session-auth REST API: issues, projects, cycles, modules, pages, webhooks, intake, scheduler occurrences, git integrations                                                                                                                                                                                                                                                                        | 125 files, ~27.7k LOC                                                                  | ~264 route/action calls / 184 view classes / 130 serializers / 0 own models (uses `db`)                                                                                                                         | 0 tasks / 0 signals; `tests/contract/app`, `tests/unit`                                                                                    | web, admin                                                             | Highest: imports `db, utils, bgtasks, runner, integrations, license, search, settings, authentication`               | **Highest** — migrate last                                                                                                                                       |
| `api` ↻           | External token REST API (`api/v1/`): mirrors core resources for third parties                                                                                                                                                                                                                                                                                                                          | 57 files, ~13.7k LOC (was 54 / ~12.5k; `work_item` URLs added)                         | ~88 / 56 / 67 / 0 own models                                                                                                                                                                                    | 0 / 0; `tests/contract/api`                                                                                                                | external API tokens                                                    | High: `app` (permissions), `api` auth middleware, `runner`, `db`, `search`, `utils`                                  | **High** — public contract, needs full parity harness                                                                                                            |
| `space`           | Public read API (`api/public/`): deploy boards, anchors, assets                                                                                                                                                                                                                                                                                                                                        | 29 files, ~2.9k LOC                                                                    | ~25 / 21 / 31 / 0 own models                                                                                                                                                                                    | 0 / 0; no dedicated contract dir                                                                                                           | space frontend                                                         | Medium: `app` (serializers), `db`, `settings` (S3), `utils`                                                          | Low-Medium — mostly reads                                                                                                                                        |
| `web`             | Edge: `robots.txt` + health check (`web/urls.py`, `web/views.py`)                                                                                                                                                                                                                                                                                                                                      | 4 files, 34 LOC                                                                        | 2 / 0 / 0 / 0                                                                                                                                                                                                   | 0 / 0 / 0                                                                                                                                  | LBs, crawlers                                                          | None                                                                                                                 | **Minimal**                                                                                                                                                      |
| `authentication`  | Sessions, CSRF token, email/magic, OAuth (google/github/gitlab/gitea), CLI device codes (`pidash auth login`), user-auth workflow                                                                                                                                                                                                                                                                      | 51 files, ~5.0k LOC                                                                    | ~37 / 43 / 0 (uses `app` serializers) / `Session`, `SocialLoginConnection`, `CLIDeviceCode` via `db`                                                                                                            | 4 mail/magic tasks via `bgtasks` / 0; `tests/smoke/test_auth_smoke.py`                                                                     | web, admin, space, desktop/CLI                                         | High: `db`, `app`, `api` (token auth), `runner` (machine tokens), `license`, `settings` (redis, S3)                  | **High** — every client; endpoints migrate late, auth _library_ ships in slice 0                                                                                 |
| `runner`          | Agent-run lifecycle: REST (`api/v1/runner/`, `api/runners/`), machine sessions, enrollment, matcher/outbox/pubsub services, diagnostics                                                                                                                                                                                                                                                                | 48 code files, ~13.8k LOC (75 files incl. migrations at first measure)                 | ~69 / 45 / 16 / 18 model classes (`runner/models.py`: `AgentRun`, `Runner`, `Pod`, `DevMachine`, `MachineToken`, sessions…)                                                                                     | 11 tasks / 1 (`runner/signals.py`) / `tests/contract/runner`, `tests/unit/runner`                                                          | runner agents, desktop, web                                            | Very high: `api`, `authentication`, `cloud_agent`, `core`, `db`, `managed_runner`, `settings` (redis)                | **High** — realtime paths; Channels consumer already a reject-stub (`runner/consumers.py`), so the WS replacement is greenfield                                  |
| `orchestration` ↻ | Dispatch engine: scheduling, state transitions, workpad, agent phases, signals (no routes)                                                                                                                                                                                                                                                                                                             | 10 files, ~3.7k LOC (was 8 / ~3.0k; `wake.py`, `workpad.py` added)                     | 1 (noise) / 0 / 0 / 0 own models                                                                                                                                                                                | 0 tasks / `orchestration/signals.py` (+ `_DISPATCH_IMMEDIATE_ATTR`, `MOVED_BY_RUN_ATTR` flags) / `tests/unit/orchestration`                | internal (runner, bgtasks, assistant call in)                          | High: `core`, `db`, `prompting`, `runner`, `utils`                                                                   | **High** — signal side effects; needs dual-run care                                                                                                              |
| `assistant` ↻     | AI assistant: threads/turns, SSE event stream, tool calls (issues, github, runs), MCP servers, BYOK llm configs (decrypt via `license`)                                                                                                                                                                                                                                                                | 37 files, code ~4.7k LOC                                                               | ~12 / 12 / ~3 + per-view / 6 model classes (`assistant/models.py`: threads, turns, events, MCP servers, llm configs)                                                                                            | 2 tasks (turn run, stale-turn sweep) / 0; `tests/contract/assistant`                                                                       | web                                                                    | High: `app`, `core`, `db`, `ee` (model provider), `managed_runner`, `search`, `settings` (redis), `license`, `utils` | **High** — streaming + secrets + spending brake (`assistant_message` 30/hour throttle)                                                                           |
| `prompting` ↻     | Prompt templates: sections, overrides, compiled/kind rendering, seed + reseed commands; **plus `fragments/` + `sections/` markdown** (prompt-content system: intro, relationships, session framing, pidash-cli, posture, autonomy, state routing, workpad template)                                                                                                                                    | 21 py files, ~3.4k LOC + markdown content dirs (was 27 files incl. migrations / ~3.1k) | ~4 / 4 / ~2 / 2 (`PromptTemplate`, `PromptSectionOverride`)                                                                                                                                                     | 0 / 0; `tests/contract/prompting`, `tests/unit/prompting` (grew: composer/context tests)                                                   | web/admin (template admin), internal (orchestration/assistant compose) | Medium: `db`, `runner` at import level (orchestration/assistant compose at runtime)                                  | Medium — the markdown content ships as data with the slice, not code                                                                                             |
| `scheduler`       | Builtin scheduler seeding on workspace create (`scheduler/signals.py`, `scheduler/builtins`)                                                                                                                                                                                                                                                                                                           | 3 files, 235 LOC                                                                       | 0 / 0 / 0 / `Scheduler`, `SchedulerBinding` live in `db`                                                                                                                                                        | 0 / 1 `post_save(Workspace)` seeder / `tests/unit/scheduler`                                                                               | internal; surfaced via `app/urls/scheduler.py` (web)                   | Low: `db` only                                                                                                       | Low — but the `post_save` seeder must not double-fire during dual-run                                                                                            |
| `bgtasks`         | Celery worker plane: ~40 task modules (mail, webhooks, sync, exports, notifications, scheduler, agent ticker, loop) + beat wiring consumers                                                                                                                                                                                                                                                            | 41 files, ~9.8k LOC                                                                    | 1 (noise) / 0 / 0 / 0                                                                                                                                                                                           | **60 of the 78 repo-wide task decorators** live here / `github_signals.py` imports signal receivers / `tests/unit/bg_tasks` | none directly (workers for all areas)                                  | Very high: `api`, `app`, `db`, `integrations`, `license`, `loop`, `orchestration`, `runner`, `settings`, `utils`     | **High** — broker/beat compatibility constrains the worker swap                                                                                                  |
| `license`         | Instance console (`api/instances/`): instance admin, configuration values, encryption helpers                                                                                                                                                                                                                                                                                                          | 30 code files, ~1.9k LOC (37 files incl. migrations at first measure)                  | ~15 / 14 / 7 / `Instance`, `InstanceAdmin` (`license/models`)                                                                                                                                                   | 1 / 0 / 0                                                                                                                                  | admin                                                                  | Low-Medium: `app` (shared helpers), `authentication`, `config`, `db`, `utils`                                        | **Low** — pilot candidate                                                                                                                                        |
| `analytics`       | Empty: `__init__.py` + `apps.py` only (13 LOC, no routes/views/models)                                                                                                                                                                                                                                                                                                                                 | 2 files, 13 LOC                                                                        | 0 / 0 / 0 / 0                                                                                                                                                                                                   | 0 / 0 / `tests/unit/analytics` (orphan — verify)                                                                                           | none                                                                   | None                                                                                                                 | None — **skip** (disposition `skip`, §6)                                                                                                                         |
| `search`          | FTS library (`search/issue.py`): `SearchVector`s pinned byte-identical to `issues_fts_idx` / `issue_comments_fts_idx` (pins verified at re-measure); no routes                                                                                                                                                                                                                                         | 2 files, 237 LOC                                                                       | 0 / 0 / 0 / 0                                                                                                                                                                                                   | 0 / 0 / `tests/unit/search`                                                                                                                | web via `app`/`api` search endpoints                                   | Low: `db` only                                                                                                       | Low — port as library with index-parity test                                                                                                                     |
| `integrations`    | Git provider adapters (github/gitlab) + code-review helpers; no routes                                                                                                                                                                                                                                                                                                                                 | 9 files, ~1.6k LOC                                                                     | 0 / 0 / 0 / 0                                                                                                                                                                                                   | 0 / 0 / `tests/unit/integrations`                                                                                                          | internal (`app` integration views, `bgtasks` sync tasks)               | Low: `db`, `license` (tokens), `utils`                                                                               | Low-Medium                                                                                                                                                       |
| `cloud_agent`     | Cloud dispatch: admission, creation, policy, events, github MCP; tasks                                                                                                                                                                                                                                                                                                                                 | 15 files, ~1.7k LOC                                                                    | 0 / 0 / 0 / 0 own models                                                                                                                                                                                        | 3 named tasks (`cloud_agent.scan_queued_runs`, `cloud_agent.sweep_stale_runs`, …) / 0                                                      | internal (orchestration/runner)                                        | Medium: `assistant`, `core`, `db`, `runner`                                                                          | Medium                                                                                                                                                           |
| `managed_runner`  | Desktop/managed execution: policy, admission, permissions (`IsDesktopSession`), tasks                                                                                                                                                                                                                                                                                                                  | 6 files, 290 LOC                                                                       | 0 / 0 / 0 / 0                                                                                                                                                                                                   | 1 (`managed_runner.expire_waiting_runs`) / 0 / `tests/unit/managed_runner`                                                                 | desktop                                                                | Medium: `core`, `runner` (↔ cycle)                                                                                   | Medium                                                                                                                                                           |
| `loop`            | Auto-PM jobs (`users/me/auto-pm/…`, `loop/urls.py`, `loop/views.py`)                                                                                                                                                                                                                                                                                                                                   | 9 files, 764 LOC                                                                       | ~5 / 5 / ~1 / `LoopJob`, `LoopTarget`, `LoopUserPreference` in `db`                                                                                                                                             | 0 (served by `bgtasks/loop.py`) / 0; `tests/contract/loop`                                                                                 | web                                                                    | Medium: `app`, `assistant`, `bgtasks`, `db`, `license`                                                               | Low                                                                                                                                                              |
| `ee` ↻            | OSS extension stubs (no routes): `ee/assistant/{model_provider,stt_provider}.py`, `ee/cloud_agent/{toolsets,model_provider}.py`, `ee/authentication/desktop.py`, `ee/settings/user_settings.py`                                                                                                                                                                                                        | 11 files, 367 LOC (was 10 / 295; `stt_provider.py` added)                              | 0 / 0 / 0 / 0                                                                                                                                                                                                   | 0 / 0 / 0                                                                                                                                  | private overlay (via stubs only — not detailed here)                   | Imported by `assistant`, `cloud_agent`, `core`                                                                       | Low — keep stub-for-stub; the Rust backend needs equivalent injection points as traits with defaults (see §4)                                                    |
| `db` ↻            | Schema owner: ~30 model modules (`db/models/`), **223 migration files (55 with `RunPython`)**, ~20 management commands (`db/management/commands/`), mixins, read-replica routing                                                                                                                                                                                                                       | 60 code files, ~7.6k LOC (235 files incl. migrations at first measure)                 | 1 (noise) / 0 / 0 / **164 class defs in model files** (~165 concrete models incl. `Issue`, `Project`, `Workspace`, `User`, `State`, `Cycle`, `Module`, `Page`, `Scheduler*`, `Webhook`, `APIToken`, `Session`…) | 0 tasks / `db/models/user.py` receivers / `tests/unit/models`                                                                              | ops (migrations, commands)                                             | Hub: everything reads `db`; `db` itself imports `bgtasks`, `core`, `license`, `utils` (signals/tasks — the cycles)   | **Structural** — exactly one side owns the schema at any time (parent rule); Rust starts read/write against the same tables, never owns migrations until cutover |
| `middleware`      | `middleware/{db_routing,logger,request_body_size}.py` + `SessionMiddleware`; enabled list in `settings/common.py` `MIDDLEWARE`                                                                                                                                                                                                                                                                         | 5 files, 362 LOC                                                                       | 0 / 0 / 0 / 0                                                                                                                                                                                                   | 0 / 0 / `tests/unit/middleware`                                                                                                            | all                                                                    | Low: `bgtasks`, `utils`                                                                                              | Low — port as axum/tower middleware                                                                                                                              |
| `utils` ↻         | Shared kit, 73 files, ~12.3k LOC (was 72 / ~11.5k; `openapi/` grew): paginators (`BasePaginator`, `GroupedOffsetPaginator`), filters (`ComplexFilterBackend`, `IssueFilterSet`), `issue_filters`, `order_queryset`, cache, host/URL, timezone, CSV, porters, github clients, duplicated permission trees (`utils/permissions/` mirrors `app/permissions/` — both trees verified present at re-measure) | 73 files, ~12.3k LOC                                                                   | ~4 (noise) / 0 / 1 / 0                                                                                                                                                                                          | 0 / 0 / `tests/unit/{serializers,permissions,utils}`                                                                                       | all                                                                    | High fan-out: `app`, `bgtasks`, `db`, `license`, `runner` (↔ cycle with `app`)                                       | Medium — port once as the shared kernel; deduplicate the twin permission trees                                                                                   |
| `core`            | 5 files, 478 LOC: `agent_execution.py` (executor routing), `permissions.py`, `querysets.py`, `user_settings.py`                                                                                                                                                                                                                                                                                        | —                                                                                      | 0 / 0 / 0 / 0                                                                                                                                                                                                   | 0 / 0 / 0                                                                                                                                  | internal                                                               | `db` at import level (`ee`, `managed_runner`, `runner` compose at runtime)                                           | Medium — executor routing is load-bearing for 6/8/9                                                                                                              |
| `config`          | 3 files, 496 LOC: `accessor.py` + `registry.py` (settings overlay mechanism)                                                                                                                                                                                                                                                                                                                           | —                                                                                      | 0 / 0 / 0 / 0                                                                                                                                                                                                   | 0 / 0 / `tests/unit/config`                                                                                                                | overlay                                                                | `license` (import-level; wider use at runtime)                                                                       | Low — keep semantics identical (see §4)                                                                                                                          |
| `settings`        | 9 files, ~1.8k LOC: `common/local/production/test/mongo/openapi/redis/storage` + celery beat schedule (`celery.py`: `beat_schedule`, `DatabaseScheduler`)                                                                                                                                                                                                                                              | —                                                                                      | 0 / 0 / 0 / 0                                                                                                                                                                                                   | beat: `celery.py` `beat_schedule` / 0 / `tests/unit/settings`                                                                              | ops                                                                    | `config`, `utils`                                                                                                    | Low — overlay-compatible settings are a migration prerequisite                                                                                                   |
| `throttles`       | `throttles/asset.py` (15 LOC; `AssetRateThrottle`, 5/min)                                                                                                                                                                                                                                                                                                                                              | —                                                                                      | 0 / 0 / 0 / 0                                                                                                                                                                                                   | 0 / 0 / 0                                                                                                                                  | asset downloads                                                        | None                                                                                                                 | Minimal                                                                                                                                                          |

Supporting totals (re-measured at `ddaef0c2`): `tests/` tree holds ~177 files / ~42k LOC
(`contract/{api,app,assistant,loop,prompting,runner}`, `unit/{…}`, `smoke/test_auth_smoke.py`);
176 `transaction.atomic` sites; signal receivers in 5 files
(`scheduler/signals.py`, `orchestration/signals.py`, `runner/signals.py`,
`bgtasks/github_signals.py`, `db/models/user.py`); 24 management-command modules
(`db` 17, `license` 2, `prompting` 4, `runner` 1); 1 Channels consumer (retired stub);
~270 serializer class defs repo-wide; ~614 route-call sites; 40 `crum` refs.
`logs/` (local runtime logs) and `seeds/data` (seed data, ships with slice 14)
are untracked/supporting, not ported areas.

## 2. Coupling between areas

Shared models: nearly all state lives in `db/models/` (`Issue`, `Project`, `Workspace`, `User`,
`State`, `Cycle`, `Module`, `Page`, `Scheduler`, `Webhook`, `APIToken`, …) plus four
out-of-`db` model homes — `runner/models.py` (18), `assistant/models.py` (6),
`prompting` models (2), `license/models` (`Instance*`). Any slice touching behavior of these
rows must assume concurrent Django writes to the same tables.

Cross-app imports (same `^from pi_dash.<area>` / `^import pi_dash.<area>` scan, re-run at
`ddaef0c2` — the shape changed: the `app`↔`api`, `orchestration`↔`prompting` and
`assistant`↔`cloud_agent` top-level cycles are gone; four remain):

```text
app            -> authentication bgtasks db integrations license runner
                  search settings throttles utils
api            -> app authentication bgtasks core db integrations runner
                  search settings utils
space          -> app authentication bgtasks db settings utils
web            -> (none)
authentication -> api app bgtasks db license runner settings utils
runner         -> api authentication cloud_agent core db managed_runner settings
orchestration  -> core db prompting runner utils
assistant      -> app core db ee integrations license managed_runner
                  search settings utils
prompting      -> db runner
scheduler      -> db
bgtasks        -> api app db integrations license loop orchestration runner
                  settings utils
license        -> app authentication config db utils
analytics      -> (none)
search         -> db
integrations   -> db license utils
cloud_agent    -> assistant core db runner
managed_runner -> core runner
loop           -> app assistant bgtasks db license
ee             -> assistant cloud_agent core
db             -> bgtasks core license utils
middleware     -> bgtasks utils
utils          -> app bgtasks db license runner
core           -> db
config         -> (none)
settings       -> config utils
throttles      -> (none)
```

Remaining cycles (so strict bottom-up ordering is still impossible — slices cut at route boundaries
against the shared DB instead): `db` ↔ `bgtasks` (signal + task imports);
`assistant` ↔ `ee`; `app` ↔ `utils`; `runner` ↔ `managed_runner`.
One-way now (previously cyclic): `orchestration` → `prompting` → `runner`;
`api` → `app`; `cloud_agent` → `assistant`; `ee`/`managed_runner` → `core` → `db`.

Signal side effects: `scheduler/signals.py` seeds builtin schedulers on `Workspace` create;
`orchestration/signals.py` dispatches agent runs (with immediate-dispatch and moved-by-run
re-entrancy guards); `runner/signals.py` + `runner/models.py` receivers finalize runs;
`bgtasks/{github_signals,scheduler}.py` bridge git/scheduler events into tasks;
`db/models/user.py` has user receivers. During dual-run, exactly one side must own each
receiver (default: Django keeps them until its slice migrates).

Shared permission classes: two parallel trees — `app/permissions/{base,project,workspace,page}.py`
(`ROLE`, `allow_permission`, `ProjectEntityPermission`, `WorkspaceEntityPermission`, …) and
`utils/permissions/` (same names), plus `utils/permissions.py` legacy shims,
`core/permissions.py` (`check_project_role`, executor roles), `runner/services/permissions.py`
(`is_workspace_member`, `can_view/manage_runner`), `managed_runner/permissions.py`
(`IsDesktopSession`), `license/api/permissions/instance.py` (`InstanceAdminPermission`).
The Rust foundation must unify these once; slices must not fork a third tree.

## 3. Which parts of Django are relied on

- ORM reads: ~4.9k queryset call sites (`.filter/.exclude/.annotate/.select_related/…`);
  664 `.annotate(` sites, `Subquery` 114, `OuterRef` 240, `Exists` 38 — concentrated in
  `app` issuelist/search paths and `utils/{issue_filters,order_queryset,grouper}.py`.
  Postgres FTS: `search/issue.py` vectors pinned to `issues_fts_idx` / `issue_comments_fts_idx`
  (7 refs, verified at re-measure).
- ORM writes: 176 `transaction.atomic` sites; custom managers (`SoftDeletionManager`,
  `IssueManager`, `StateManager`, `TriageStateManager` in `db/models/{issue,state}.py`);
  `BaseModel.save()` auto-stamps `created_by/updated_by` via **django-crum** (40 refs).
  `select_for_update` 118 sites, `transaction.on_commit` 107 sites.
- `contrib.auth` (20 refs): custom `User(AbstractBaseUser, PermissionsMixin)`, groups/permissions,
  `AuthenticationMiddleware`. Sessions: custom `Session` model + `DBSessionStore` +
  `authentication/middleware/session.py` `SessionMiddleware` (153 `session` refs).
- CSRF: `CsrfViewMiddleware` + `auth/get-csrf-token/` endpoint (21 refs) — required by web/admin.
- **django-filter** (`FilterSet` 28, `ComplexFilterBackend`, `IssueFilterSet` in `utils/filters.py`).
- Pagination: custom `utils/paginator.py` + `utils/global_paginator.py` (no DRF built-in pagination
  in use). Throttles: DRF defaults (`AnonRateThrottle`, anon 30/min) plus per-scope rates in
  `settings/common.py` (`user` 120/min, `asset_id` 5/min, `runner_chat_send` 60/min,
  `auth_device_start` 20/min, `assistant_message` 30/hour, `assistant_llm_test` 6/min — all
  verified present) and `throttles/asset.py`, `authentication/rate_limit.py`, `api/rate_limit.py`.
- **drf-spectacular** (35 refs, `settings/openapi.py`, `ENABLE_DRF_SPECTACULAR` flag, `api/schema/…`
  routes, `utils/openapi/` decorators).
- **storages**: `S3Storage` (`settings/storage.py`) for file assets/descriptions.
- **Channels 4.1**: one consumer, already a reject-stub (`runner/consumers.py`, `runner/routing.py`
  `ws/runner/`) — do not port (disposition `skip`, §6); the replacement realtime path keeps the
  close-code-1008 behavior old runners rely on (see slice 10).
- **Celery 5.4 + django-celery-beat**: 78 task decorators (60 in `bgtasks/`), `celery.py`
  `beat_schedule` with `DatabaseScheduler`; `settings/redis.py` (`redis_instance`,
  `async_redis_instance`) backs pubsub/outbox (`runner/services/{pubsub,outbox}.py`) and
  `assistant` SSE (`assistant/views/events.py`, `assistant/runtime/events.py`).
- Management commands: 24 modules (list in §1) — ops tooling (`wait_for_db`,
  `create_instance_admin`, `reseed_*`, `ensure_project_pods`, …) migrates in the ops tail.
- Read replicas: `middleware/db_routing.py` (+ `ReadReplicaRoutingMiddleware`, `ReadReplicaControlMixin`)
  — the Rust data layer needs the same routing or explicit primary pinning per slice.

## 4. OSS extension points the overlay relies on (public surface only)

99 files outside OSS import Django or `pi_dash` at the time of writing (file count only;
no overlay contents are described here, and the count drifts with development). The OSS surface the Rust backend must keep equivalent:

1. Settings overlay: `settings/{common,local,production,test,mongo,openapi,redis,storage}.py`
   composed via `config/{accessor,registry}.py` (`get_config`) — env-driven, additive overrides.
   Rust: app builder composed in the private crate's `main.rs`.
2. `ee/` stubs (7 modules now, incl. `ee/assistant/stt_provider.py`): `ee/assistant/model_provider.py`,
   `ee/cloud_agent/{toolsets,model_provider}.py`, `ee/authentication/desktop.py`,
   `ee/settings/user_settings.py` — import-stable override points; Rust needs the same
   seams as traits with default implementations, plus an explicit map for the private crate.
3. URL conf: `urls.py` include structure (table at top) — slice routing keys off these prefixes.
4. Auth classes: DRF `SessionAuthentication` default; `api/middleware/api_authentication.py`
   (`APIKeyAuthentication` over `APIToken`); `app/middleware/api_authentication.py`;
   `runner/authentication.py` (`MachineTokenAuthentication`, runner access tokens);
   `authentication/session.py` (`BaseSessionAuthentication`, incl. space variants).
   Rust: session reader + PBKDF2 verification per the technical baseline.
5. Middleware chain (`settings/common.py` `MIDDLEWARE`): CORS, security, whitenoise, session,
   common, CSRF, contrib-auth, clickjacking, crum, gzip, body-size limit, token log, request log —
   each needs an axum/tower equivalent or a documented drop.
6. Management-command pattern (`*/management/commands/*.py`) and the `S3Storage` / beat-scheduler
   settings keys, which overlay ops tooling invokes.

## 5. Dependency graph

```text
                        ┌─────────┐
                        │   web   │  edge (no deps)
                        └─────────┘
 ┌──────┐  ┌────────┐  ┌─────────┐  ┌─────────┐
 │license│  │  space │  │   api   │  │   app   │  API surfaces (migrate late→last)
 └──┬───┘  └──┬─────┘  └────┬────┘  └────┬────┘
    │         │             │            │
 ┌──┴─────────┴──┐  ┌───────┴────────────┴───┐
 │authentication │  │ runner  orchestration  │  engines (auth endpoints late,
 └───────────────┘  │ assistant loop        │  engine libs mid)
                    └───────┬────────────────┘
            ┌───────────────┼───────────────┐
            │ prompting scheduler bgtasks   │  services + worker plane
            │ integrations search(lib)      │  (+ prompting content-as-data)
            └───────────────┬───────────────┘
        ┌───────────────────┴──────────────────┐
        │ cloud_agent managed_runner core ee   │  execution routing + stubs
        └───────────────────┬──────────────────┘
   ┌────────────────────────┴────────────────────────┐
   │ db utils middleware settings config throttles   │  shared kernel (port once, slice 0)
   └─────────────────────────────────────────────────┘
```

Reads flow downward (upper layers import lower ones); the four remaining cycles in §2 mean the arrows are not
acyclic — hence route-boundary slices over one shared Postgres/Redis, with exactly one schema
owner (Django, until cutover).

## 6. Migration slices in recommended order

Each slice is independently routable and rollback-able (routing change, per parent rule).
Order reasons: leaves before hubs, libraries before callers, realtime/auth/core last; every slice
reuses the foundation (§6a) instead of forking helpers. A slice is an **ordering unit**, not a
filing unit: the filing units are the foundation issues (§6a) and the domain epics (§6b), and
several domains make up the large slices.

| #   | Slice (prefix / area)                                                                                | Domains (§6b)                     | Disposition | Why this position                                                                                                                                                                                                                                           |
| --- | ---------------------------------------------------------------------------------------------------- | --------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0   | Foundation + `web/` edge                                                                             | F-01 … F-10 (§6a); `web/` is D-00 | `full`      | Unblocks everything; zero product risk; proves the rollback path (routing change) on day one                                                                                                                                                                |
| 1   | `license` — `api/instances/` (**first pilot**, §6c)                                                  | D-01                              | `full`      | Small vertical that exercises DB R/W + auth + config overlay end to end                                                                                                                                                                                     |
| 2   | `space` — `api/public/`                                                                              | D-02                              | `full`      | Isolated client; proves public/weak-auth path; no core-model writes (verify per endpoint)                                                                                                                                                                   |
| 3   | `loop` — auto-pm endpoints                                                                           | D-03                              | `full`      | Small; exercises `assistant`/`bgtasks` coupling in miniature before those slices                                                                                                                                                                            |
| 4   | `prompting` + `scheduler`                                                                            | D-04                              | `full`      | Pure service; settle the signal-ownership rule (Django keeps seeder until cutover) while blast radius is tiny                                                                                                                                               |
| 5   | `integrations` (library) + git sync task parity                                                      | D-05                              | `full`      | No routes; must precede `app`/`api` slices that render linked PR/review data                                                                                                                                                                                |
| 6   | `assistant` — `api/…/ai-assistant/` + MCP client + SSE                                               | D-06                              | `full`      | First stateful/LLM slice; needs slices 0–1 (redis, license-decrypt) and 5 (github tools)                                                                                                                                                                    |
| 7   | `bgtasks` worker plane                                                                               | D-07 … D-10                       | `full`      | Workers must exist before engine slices move dispatch targets; per-task-group routing keeps rollback routing-only. During coexistence Rust enqueues Celery-format messages (general design, ~100 lines) so Python workers keep serving unported task groups |
| 8   | `cloud_agent` + `managed_runner`                                                                     | D-11                              | `full`      | Depends on 6 (assistant runs) and 7 (sweep/scan tasks)                                                                                                                                                                                                      |
| 9   | `orchestration` (engine, no routes)                                                                  | D-12                              | `full`      | Depends on 4, 7, 8; dual-run with one receiver-owner (Django until this slice cuts over)                                                                                                                                                                    |
| 10  | `runner` — `api/v1/runner/`, `api/runners/`, realtime                                                | D-13 … D-15                       | `full`      | Hardest realtime surface; goes after its dispatch/finalization dependencies (7–9)                                                                                                                                                                           |
| 11  | `authentication` endpoints — `auth/`                                                                 | D-16, D-17                        | `full`      | Blast radius is every client; the _mechanism_ ships in F-05, endpoints move here once all consumers tolerate it                                                                                                                                             |
| 12  | `api` — `api/v1/` external API                                                                       | D-18 … D-23                       | `full`      | Third-party contract; needs the full contract suite and slices 5, 6, 10, 11 beneath it                                                                                                                                                                      |
| 13  | `app` — `api/` core session API                                                                      | D-24 … D-36                       | `full`      | Largest, hottest, most coupled — last product slice by design                                                                                                                                                                                               |
| 14  | Ops tail + cutover                                                                                   | D-37                              | `full`      | Only when every route is served by Rust (parent "Done when")                                                                                                                                                                                                |
| —   | `analytics/`, `ws/runner/` stub (`runner/consumers.py`, `runner/routing.py`), `debug_toolbar` wiring | —                                 | `skip`      | Dead/retired/dev-only (§7); never filed, excluded from contract-test scope                                                                                                                                                                                  |

### 6a. Foundation issues (slice 0 — filed one by one, Rich reviews every PR)

These are single issues, not epics; they are not split by agents. Each is `blocked_by` the
one above it unless noted. Filed after the stage-2 design decisions are on the wiki.

| Id   | Issue                                       | Delivers                                                                                                                                                                                                                                                                 | Blocked by |
| ---- | ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------- |
| F-01 | Rust workspace scaffold                     | `rust-api/` with its own `[workspace]`, excluded from the root Cargo workspace; axum app; `serve`/`worker` modes; `#![forbid(unsafe_code)]`; CI on the directory                                                                                                         | —          |
| F-02 | Router-by-prefix proxy + `web/` edge (D-00) | Caddy/proxy routing table keyed on the §0 prefix map, per-prefix flip flag default-off, `robots.txt` + health check served by Rust, rollback drill recorded                                                                                                              | F-01       |
| F-03 | Config + settings overlay equivalent        | `get_config` semantics (env or DB `InstanceConfiguration`, Fernet), app builder the private crate composes in its own `main.rs` (§4 item 1)                                                                                                                              | F-01       |
| F-04 | Data layer kernel                           | sqlx pools (primary + read replica routing), `sea-query` for dynamic filters, soft-delete views, explicit audit/tenant request context, transaction wrapper with post-commit actions                                                                                     | F-01       |
| F-05 | Auth library                                | Django session reader (signed cookie + `Session` table, dual `admin-session-id` cookie), PBKDF2 verification, `APIToken`/`MachineToken`/runner JWT validation, CSRF token endpoint semantics                                                                             | F-04       |
| F-06 | Permission kernel                           | One tree replacing `app/permissions/`, `utils/permissions/`, `core/permissions.py`, `runner/services/permissions.py`, `managed_runner/permissions.py`, `license/api/permissions/`; `allow_permission` as an extractor; workspace-scoped DB handle by construction        | F-04, F-05 |
| F-07 | Serializer + paginator kernel               | DRF-compatible JSON (null vs absent, datetime/Decimal formats), `expand`/`fields` mechanism, `OffsetPaginator` / `GroupedOffsetPaginator` / `SubGroupedOffsetPaginator` with identical cursor format, `ComplexFilterBackend` + `IssueFilterSet` + legacy `issue_filters` | F-04       |
| F-08 | Middleware chain                            | tower equivalents for the `MIDDLEWARE` list in `settings/common.py` (CORS, security headers, gzip, body-size cap, request/token logging, session)                                                                                                                        | F-05       |
| F-09 | Job queue + scheduler loop                  | Postgres-backed queue with transactional enqueue, worker mode, beat-equivalent loop, Celery-format publisher for coexistence                                                                                                                                             | F-04       |
| F-10 | Extension seams for the overlay             | Traits with default impls for the 7 `ee/` stubs (§4 item 2), named route-group replacement in the app builder (§4 item 3), private migration directory convention                                                                                                        | F-03, F-06 |

### 6b. Domain epics (the filing units for stages 4–5)

One row = one epic. `Sources` are the Python paths the epic ports (all under `apps/api/pi_dash/`);
`Tests today` are the existing pytest files that describe behaviour (they are Django-test-client
tests and are **not** the gate — the gate is the stage-1 HTTP contract-test issue for the domain,
see §8). `Ported from` is set at filing time to the OSS `main` sha the agent ports from
(initially `ddaef0c2`); the Drift scheduler advances it. Route counts are `path(`/`re_path(`
sites in the URL module; LOC is the view package.

**Slices 0–11: one or a few domains each**

| Id   | Slice | Domain                                                | Prefix / routes                                 | Sources                                                                                                                                                                                                                                                                                      | Models                                                          | Tests today                                                                   | Disposition                                                                                |
| ---- | ----- | ----------------------------------------------------- | ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| D-00 | 0     | web edge                                              | `/` · 2                                         | `web/`                                                                                                                                                                                                                                                                                       | —                                                               | —                                                                             | `full` (ships with F-02)                                                                   |
| D-01 | 1     | license / instance console                            | `api/instances/` · 15                           | `license/`                                                                                                                                                                                                                                                                                   | `Instance`, `InstanceAdmin`, `InstanceConfiguration`            | none                                                                          | `full`                                                                                     |
| D-02 | 2     | space                                                 | `api/public/` · 25                              | `space/`                                                                                                                                                                                                                                                                                     | reads `db` (deploy boards, anchors, assets)                     | none                                                                          | `full`                                                                                     |
| D-03 | 3     | loop                                                  | `api/users/me/auto-pm/` · 5                     | `loop/`, `bgtasks/loop.py`                                                                                                                                                                                                                                                                   | `LoopJob`, `LoopTarget`, `LoopUserPreference`                   | `tests/contract/loop`                                                         | `full`                                                                                     |
| D-04 | 4     | prompting + scheduler seeding                         | `api/…/prompt-templates/` · 4                   | `prompting/` (code + `fragments/`, `sections/` as data), `scheduler/`                                                                                                                                                                                                                        | `PromptTemplate`, `PromptSectionOverride`, `Scheduler*` seeding | `tests/contract/prompting`, `tests/unit/prompting`, `tests/unit/scheduler`    | `full`                                                                                     |
| D-05 | 5     | integrations library + git sync                       | — (library)                                     | `integrations/`, `bgtasks/{git_sync_task,github_sync_task,github_signals}.py`                                                                                                                                                                                                                | reads `db`                                                      | `tests/unit/integrations`                                                     | `full`                                                                                     |
| D-06 | 6     | assistant                                             | `api/…/ai-assistant/` · 12                      | `assistant/` (views, runtime, tools, crypto), `ee/assistant/*` seams                                                                                                                                                                                                                         | `assistant/models.py` (6)                                       | `tests/contract/assistant`                                                    | `full` — the design issue decides Rust runtime vs Python sidecar before this epic is filed |
| D-07 | 7     | bgtasks: mail + notifications                         | — (tasks)                                       | `bgtasks/{email_notification_task,notification_task,magic_link_code_task,forgot_password_task,user_activation_email_task,user_deactivation_email_task,user_email_update_task,project_add_user_email_task,project_invitation_task,workspace_invitation_task}.py`                              | reads `db`                                                      | `tests/unit/bg_tasks`                                                         | `full`                                                                                     |
| D-08 | 7     | bgtasks: webhooks + activity + logging                | — (tasks)                                       | `bgtasks/{webhook_task,issue_activities_task,issue_automation_task,work_item_link_task,recent_visited_task,page_transaction_task,logger_task,event_tracking_task}.py`                                                                                                                        | reads `db`                                                      | `tests/unit/bg_tasks`                                                         | `full`                                                                                     |
| D-09 | 7     | bgtasks: cleanup, versions, exports, deletion         | — (tasks)                                       | `bgtasks/{cleanup_task,deletion_task,export_task,exporter_expired_task,analytic_plot_export,issue_version_sync,issue_description_version_sync,issue_description_version_task,page_version_task,file_asset_task,storage_metadata_task,copy_s3_object,workspace_seed_task,dummy_data_task}.py` | reads `db`                                                      | `tests/unit/bg_tasks`                                                         | `full`                                                                                     |
| D-10 | 7     | bgtasks: agent ticker + scheduler + loop workers      | — (tasks + beat)                                | `bgtasks/{agent_ticker,scheduler,loop,_rrule}.py`, `celery.py` beat schedule                                                                                                                                                                                                                 | `IssueAgentTicker`, `Scheduler*`                                | `tests/unit/bg_tasks`, `tests/unit/test_celery_schedule.py`                   | `full`                                                                                     |
| D-11 | 8     | cloud_agent + managed_runner                          | — (dispatch)                                    | `cloud_agent/`, `managed_runner/`, `core/agent_execution.py`, `ee/cloud_agent/*` seams                                                                                                                                                                                                       | reads `runner` models                                           | `tests/unit/cloud_agent`, `tests/unit/managed_runner`                         | `full`                                                                                     |
| D-12 | 9     | orchestration engine                                  | — (signals)                                     | `orchestration/` (incl. `blockers.py`, `wake.py`, `workpad.py`), `core/` remainder                                                                                                                                                                                                           | reads `db`, `runner`                                            | `tests/unit/orchestration`                                                    | `full`                                                                                     |
| D-13 | 10    | runner: enrollment + auth + machines                  | `api/v1/runner/` (enrollment, tokens, machines) | `runner/authentication.py`, `runner/services/{tokens,validation,runner_delete,pod_naming}.py`, `runner/views/{enrollment,register,runners,pods,projects,desktop,machine_commands}.py`                                                                                                        | `Runner`, `DevMachine`, `MachineToken`, `RunnerConnection`      | `tests/contract/runner`, `tests/unit/runner`                                  | `full`                                                                                     |
| D-14 | 10    | runner: sessions, long-poll, outbox, pubsub           | `api/v1/runner/` (session poll)                 | `runner/views/{sessions,machine_sessions}.py`, `runner/services/{session_service,outbox,machine_outbox,pubsub,matcher}.py`                                                                                                                                                                   | `RunnerSession`, `MachineSession`                               | same                                                                          | `full` — keeps close-code-1008 semantics for old runners; Channels stub itself is `skip`   |
| D-15 | 10    | runner: runs, approvals, chat, web management         | `api/runners/` + run endpoints                  | `runner/views/{runs,run_endpoints,approvals,chat,metrics}.py`, `runner/services/{run_lifecycle,agent_run_finalization,chat,scheduler_hook,permissions}.py`, `runner/tasks.py`, `runner/signals.py`                                                                                           | `AgentRun`, `Pod`, approvals                                    | same                                                                          | `full`                                                                                     |
| D-16 | 11    | authentication: session, email, magic, password, CSRF | `auth/` (+ `auth/spaces/…` variants)            | `authentication/{views,provider/credentials,adapter,middleware}`                                                                                                                                                                                                                             | `Session`, `User`                                               | `tests/smoke/test_auth_smoke.py`, `tests/contract/app/test_authentication.py` | `full`                                                                                     |
| D-17 | 11    | authentication: OAuth + CLI device flow               | `auth/` (google/github/gitlab/gitea, device)    | `authentication/provider/oauth/`, `authentication/views/cli/`, `authentication/services/cli_tokens.py`                                                                                                                                                                                       | `SocialLoginConnection`, `CLIDeviceCode`                        | same                                                                          | `full`                                                                                     |

**Slice 12 — `api/v1/` external API, by URL module (`api/urls/*.py`, views in `api/views/`, serializers in `api/serializers/`)**

| Id   | Domain                               | URL modules · routes                                                                       | Sources                                                                                                                   | Tests today                                                                                      | Disposition                                                                          |
| ---- | ------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| D-18 | work items                           | `work_item` 38, `label` 2, `page` 3 (page routes landed after measurement, PDASHOSS01-185) | `api/views/issue.py`, `api/serializers/issue.py`, plus the page views/serializers that landed on `main` after measurement | `tests/contract/api/{test_issue_search,test_move_endpoint,test_workpad_endpoint,test_labels}.py` | `full`                                                                               |
| D-19 | projects, members, states, estimates | `project` 4, `member` 5, `invite` 1, `user` 1, `state` 2, `estimate` 3                     | `api/views/{project,member,invite,user,state,estimate}.py` + serializers                                                  | `test_project_identifier_routing.py`                                                             | `full`                                                                               |
| D-20 | cycles + modules                     | `cycle` 8, `module` 7                                                                      | `api/views/{cycle,module}.py` + serializers                                                                               | `test_cycles.py`                                                                                 | `full`                                                                               |
| D-21 | assets, stickies, intake             | `asset` 6, `sticky` 1, `intake` 2                                                          | `api/views/{asset,sticky,intake}.py` + serializers                                                                        | none                                                                                             | `full`                                                                               |
| D-22 | git links + runner v1                | `auth` 6 (PR link, code review), `runner` 1                                                | `api/views/{github_pr,git_code_review,runner}.py`                                                                         | `test_github_pr_link.py`, `test_git_code_review_link.py`                                         | `full`                                                                               |
| D-23 | OpenAPI schema                       | `schema` 3                                                                                 | `settings/openapi.py`, `utils/openapi/`                                                                                   | none                                                                                             | `minimal` — `utoipa`-generated schema; byte-identity not required, route coverage is |

**Slice 13 — `api/` core session API, by URL module (`app/urls/*.py`, views in `app/views/<module>/`, serializers in `app/serializers/<module>.py`)**

| Id   | Domain                                                                  | URL modules · routes                             | View LOC | Models imported | Tests today                                                                                                                              | Disposition                                                       |
| ---- | ----------------------------------------------------------------------- | ------------------------------------------------ | -------- | --------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| D-24 | workspace + users + API tokens                                          | `workspace` 45, `user` 16, `api` 2, `timezone` 1 | 3,623    | 21              | `tests/contract/app/{test_workspace_app,test_workspace_join_request_app,test_profile_settings,test_api_token}.py`                        | `full`                                                            |
| D-25 | project + states + estimates                                            | `project` 20, `state` 4, `estimate` 5            | 1,624    | 12              | `test_project_app.py`, `test_move_endpoint.py`                                                                                           | `full`                                                            |
| D-26 | issues (list, detail, sub-issues, relations, activity, drafts, archive) | `issue` 45                                       | 3,694    | 19              | `test_move_endpoint.py`; `tests/unit/serializers`                                                                                        | `full` — the foundation pilot (§6c) ports the list endpoint first |
| D-27 | cycles                                                                  | `cycle` 14                                       | 1,984    | 9               | none                                                                                                                                     | `full`                                                            |
| D-28 | modules                                                                 | `module` 13                                      | 1,757    | 5               | none                                                                                                                                     | `full`                                                            |
| D-29 | views + search                                                          | `views` 7, `search` 3                            | 1,290    | 4               | `test_global_search.py`, `tests/unit/search`                                                                                             | `full` — FTS parity: identical `EXPLAIN` on `issues_fts_idx`      |
| D-30 | pages                                                                   | `page` 11                                        | 670      | 1               | none                                                                                                                                     | `full`                                                            |
| D-31 | assets                                                                  | `asset` 18                                       | 921      | 4               | none                                                                                                                                     | `full` — S3/MinIO presigned flows                                 |
| D-32 | intake                                                                  | `intake` 10                                      | 637      | 1               | none                                                                                                                                     | `full`                                                            |
| D-33 | integrations, webhooks, external                                        | `integration` 17, `webhook` 4, `external` 3      | 1,752    | 7               | `test_github_app_integration.py`, `test_github_disconnect_generic_binding.py`, `test_github_pr_link_app.py`, `test_github_pr_webhook.py` | `full`                                                            |
| D-34 | notifications                                                           | `notification` 7                                 | 308      | 0               | none                                                                                                                                     | `full`                                                            |
| D-35 | analytics + exporters                                                   | `analytic` 13, `exporter` 1                      | 1,257    | 3               | none                                                                                                                                     | `full` for analytics; exporter `minimal` pending open question 3  |
| D-36 | scheduler occurrences + bindings                                        | `scheduler` 5                                    | 502      | 4               | none                                                                                                                                     | `full`                                                            |

**Slice 14**

| Id   | Domain                                                               | Sources                                                                           | Disposition                                            |
| ---- | -------------------------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------ |
| D-37 | ops tail: management commands, seeds, replica routing, image cutover | `*/management/commands/` (24), `seeds/`, `middleware/db_routing.py`, deploy files | `full` — filed only when every route is served by Rust |

Count: 10 foundation issues + 38 domain epics. Stages 0–3 file F-01…F-10, D-00, D-01 and the
foundation pilot by hand; stage 4 trial files three epics (recommended: D-02, D-03, D-27);
stage 5 files the rest.

### 6c. Recommended pilot slices

**First pilot: D-01 (`license`, `api/instances/`).**

- Small: ~1.9k LOC, 15 routes, 14 views, 7 serializers, 2 models — reviewable in one PR.
- Low risk: admin-only client; no issue/project hot paths; no signals, no realtime, no FTS.
- Real traffic: every self-host and cloud instance console hits it (instance check/configuration),
  so parity gaps show up immediately without endangering collaboration workflows.
- Few model dependencies: `Instance`, `InstanceAdmin` (+ shared `User` reads); small enough that the
  foundation (auth library, permission kernel, settings overlay, S3/redis access) gets
  exercised end to end before larger slices depend on it.
- Success bar: byte-equivalent responses on the contract suite for all 15 routes, dual-run with
  Django owning the two tables, rollback by routing change only.

**Second pilot: the issue-list endpoint of D-26 end to end on the foundation** (general design,
stage 3). It proves the crate graph (types → db → services → api), exercises the four hardest
foundation pieces at once (filters, grouped pagination, `expand`, soft delete + scoping), and
drafts the porting guide from its own code. It starts on a partial guide and feeds it back; it
needs the proxy drill only for its cutover step and the Celery bridge only if it enqueues a task.
It also produces the first fixtures (§9) so the split template has a real example to point at.

## 7. Not ported (disposition `skip`)

Nothing is deleted in this project (parent rule 1: Python is never deleted; removal is a separate
follow-up plan after Rust serves all traffic). `skip` means: no epic is filed, no contract tests
are written, and the proxy never routes the prefix to Rust.

- `analytics/` — empty app (13 LOC, no routes/views/models/signals). The orphan
  `tests/unit/analytics` dir is likewise out of scope.
- `ws/runner/` + `runner/consumers.py` + `runner/routing.py` — control-plane WebSocket already
  retired to a reject-stub. The replacement realtime path is D-14, which preserves the
  close-code-1008 behaviour old runners rely on.
- `debug_toolbar` wiring (`urls.py` `DEBUG` branch, `settings/local.py`) — dev-only.
- Migrations history (223 files) is not ported by any slice: Django keeps owning the schema
  until the slice-14 cutover, per the one-schema-owner rule.
- Provisional `minimal` (decide before filing, open question 3): `ExporterHistory`/`Importer`
  porters (`app/urls/exporter.py`, `bgtasks/export_task.py`, `utils/porters/`) — live but
  low-traffic. The legacy non-FTS search flag (`include_comments=False` callers in `app`) —
  keep behaviour in D-29, do not carry the flag past cutover.

## 8. Per-domain gate, parity and rollback

- **The gate is an HTTP contract suite, not the existing pytest tree.** Of the 50 files under
  `tests/contract/`, at most 4 drive a live HTTP server; the rest use Django's test client and
  cannot run against a Rust process. Stage 1 (general design) files one contract-test issue per
  domain group; each domain epic names its contract-test issue, and the epic's gate sub-issue
  passes when that suite is green against both backends through the proxy. Coverage floor per
  domain: every drf-spectacular endpoint gets a shape assertion, one denied-permission case, one
  tenant-isolation case, and the suite must fail when a permission class is deliberately removed.
  The `Tests today` column in §6b is reading material for the contract-test author, nothing more.
- FTS domains assert identical `EXPLAIN` index usage (`issues_fts_idx`, `issue_comments_fts_idx`).
  Throttle scopes and CSRF/session semantics are asserted per route, not assumed.
- Rollback of any domain is a routing change back to Django (no redeploy of old code), with both
  backends on the same Postgres/Redis and Django owning the schema until D-37.
- Sub-issues do not write their own designs; they follow the porting guide and answer to their
  layer fixtures (§9); the gate answers to the contract suite.

## 9. How conversion issues are filed from this document

This section is the filer's contract. Filing is done by a human or an agent run through the
`pidash` CLI (`issue create --parent --description-file`, `issue relate`) or the MCP connector,
reading §6a/§6b directly. Nothing is generated from a spreadsheet.

**Foundation issues (F-01 … F-10) and pilots** are filed by hand, one at a time, in Backlog under
`PIDASHCONV-1`, with `blocked_by` as in §6a. They are single issues; agents do not split them.

**Domain epics (D-nn)** are filed per stage (stage 4: three; stage 5: the rest), in Backlog under
`PIDASHCONV-1`, one epic per §6b row. The epic body carries, verbatim from the row plus the
stage context:

- `Domain:` id and name; `Slice:` number; `Prefix / routes:`; `Disposition:` (`minimal` rows
  say what is narrowed).
- `Ported from:` the OSS `main` sha at filing (the drift record).
- `Python sources:` the row's paths; `Rust output:` the module paths under `rust-api/` this
  domain owns — the only paths its sub-issues may touch; foundation crates are read-only.
- `Contract tests:` the stage-1 contract-test issue id for this domain (the gate).
- `Reference:` the wiki pages to read (Start here, Porting guide, Semantic traps) with their
  `updated_at`, and the pilot files that show each pattern.
- `Split instructions:` see below.
- `Depends on:` the epics or foundation issues this one is `blocked_by` (§6 order), mirrored as
  `blocked_by` edges with `pidash issue relate`.

An epic never enters In Progress itself; it is a container. **The Release scheduler moves the
epic's sub-issues** to In Progress as their blockers finish.

**Splitting happens in the epic's first run, not at filing.** When the stage opens, one agent run
is dispatched against the epic in _split_ mode (its body says: do not implement; read the
sources; produce the sub-issue tree; stop). The agent, having read the code, creates under the
epic — with `pidash issue create --parent <epic>` and `pidash issue relate --blocked-by` — in
this order:

1. **Fixture sub-issue** (first): record the Python behaviour for every unit the domain ports —
   SQL + result rows for queries, golden input/output for serializers and guards, column lists
   for models, DB before/after for tasks — committed under `rust-api/fixtures/<domain>/` with a
   trace line per fixture naming the Python source lines. All layer sub-issues are
   `blocked_by` it.
2. **Layer sub-issues**, bottom-up: serializers → models → queries → permissions/throttles →
   tasks → handlers. Each carries ≤5 units or one topological closure, is sized for one run,
   names its fixture ids as `Done when`, and is `blocked_by` the layer below. Siblings within a
   layer are related as such.
3. **Domain gate** (last): `blocked_by` every layer sub-issue; runs the domain's contract suite
   against both backends; files fix sub-issues (under the epic, no blockers) for failures and
   re-enters In Test when they are Done.

Every sub-issue is self-contained (parent rule 8): domain, `Ported from`, its sources, its Rust
paths, fixture ids, reference pages with `updated_at`, exact `Done when`. The split run ends by
posting the tree as a comment on the epic and moving the epic to In Review, where a fresh review
run checks the split against the sources (missing units, wrong order, oversized sub-issues) before
any sub-issue is released. Rich reads the split comment for the three stage-4 epics and adjusts
the split instructions in the wiki if a pattern of bad splits appears; stage-5 splits are audited
by sampling.

**Re-syncs.** When OSS `main` moves past an epic's `Ported from` sha in a way that touches its
`Python sources` (or a migration lands on its tables), the Drift scheduler files under the epic a
re-sync sub-issue whose body lists the changed files and the affected fixture ids, `blocked_by`
nothing; it regenerates the affected fixtures and re-ports the affected units. `Ported from`
advances when it reaches Done.

## Open questions for Rich

1. Pilot choice: D-01 (`license`) first (recommended, §6c), then the issue-list foundation pilot —
   confirm, or swap D-02 (`space`, more traffic, read-only) into the first slot?
2. ~~Unify the twin permission trees?~~ Decided in the general design: F-06 unifies them once in
   the foundation; Django's two trees stay untouched (never deleted) and keep serving unported
   routes during dual-run.
3. Exporter/importer porters (D-35, `minimal`): still supported product surface, or excluded from
   the port (`skip`)? Decide before D-35 is filed.
4. Stage-4 trial epics: D-02, D-03 and D-27 are proposed (one read-only surface, one small
   task-coupled surface, one mid-sized `app` domain). Confirm or substitute.
