# Django backend inventory and Rust migration slices

Status: **draft for review** (PIDASHCONV-2). Analysis only — no code.
Public document: describes OSS extension points only, never private overlay internals.

**Source of truth for issue creation.** Conversion issues are filed directly
from the slice tables in §6 and the issue-creation rules in §9. There is no TSV
middleman: no `GRAPH.tsv`, no `MODELS.tsv`, no `ENDPOINTS.tsv`. Where the
general design doc names those files, this document supersedes it — the tables
below, with the `Disposition` column (`full` | `minimal` | `skip`), are what
the filer reads.

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

| Prefix | Module | Clients |
|---|---|---|
| `api/` | `app.urls` (22 modules) | web, admin (session auth) |
| `api/` | `assistant.urls`, `loop.urls`, `prompting.urls` | web (AI assistant, auto-pm, prompt admin) |
| `api/public/` | `space.urls` | space frontend (public deploy boards) |
| `api/instances/` | `license.urls` | admin (instance console) |
| `api/runners/` | `runner.web_urls` | web (agent-run management, session auth) |
| `api/v1/` | `api.urls` (16 modules) | external API tokens |
| `api/v1/runner/` | `runner.urls` | runner agents (machine tokens), desktop |
| `auth/` | `authentication.urls` | web, admin, space (`spaces/…`), desktop/CLI (device flow) |
| `/` | `web.urls` (`robots.txt`, health check) | load balancers, crawlers |
| `ws/runner/` | `runner/routing.py` | retired — stub consumer rejects all traffic |

## 1. Area-by-area inventory

Size = non-migration, non-test Python LOC in the area directory.

| Area | What it is | Size | Routes / Views / Serializers / Models | Tasks / Signals / Tests | Main clients | Coupling | Risk |
|---|---|---|---|---|---|---|---|
| `app` | Core session-auth REST API: issues, projects, cycles, modules, pages, webhooks, intake, scheduler occurrences, git integrations | 125 files, ~27.7k LOC | ~264 route/action calls / 184 view classes / 130 serializers / 0 own models (uses `db`) | 0 tasks / 0 signals; `tests/contract/app`, `tests/unit` | web, admin | Highest: imports `db, utils, bgtasks, runner, integrations, license, search, settings, authentication` | **Highest** — migrate last |
| `api` ↻ | External token REST API (`api/v1/`): mirrors core resources for third parties | 57 files, ~13.7k LOC (was 54 / ~12.5k; `work_item` URLs added) | ~88 / 56 / 67 / 0 own models | 0 / 0; `tests/contract/api` | external API tokens | High: `app` (permissions), `api` auth middleware, `runner`, `db`, `search`, `utils` | **High** — public contract, needs full parity harness |
| `space` | Public read API (`api/public/`): deploy boards, anchors, assets | 29 files, ~2.9k LOC | ~25 / 21 / 31 / 0 own models | 0 / 0; no dedicated contract dir | space frontend | Medium: `app` (serializers), `db`, `settings` (S3), `utils` | Low-Medium — mostly reads |
| `web` | Edge: `robots.txt` + health check (`web/urls.py`, `web/views.py`) | 4 files, 34 LOC | 2 / 0 / 0 / 0 | 0 / 0 / 0 | LBs, crawlers | None | **Minimal** |
| `authentication` | Sessions, CSRF token, email/magic, OAuth (google/github/gitlab/gitea), CLI device codes (`pidash auth login`), user-auth workflow | 51 files, ~5.0k LOC | ~37 / 43 / 0 (uses `app` serializers) / `Session`, `SocialLoginConnection`, `CLIDeviceCode` via `db` | 4 mail/magic tasks via `bgtasks` / 0; `tests/smoke/test_auth_smoke.py` | web, admin, space, desktop/CLI | High: `db`, `app`, `api` (token auth), `runner` (machine tokens), `license`, `settings` (redis, S3) | **High** — every client; endpoints migrate late, auth *library* ships in slice 0 |
| `runner` | Agent-run lifecycle: REST (`api/v1/runner/`, `api/runners/`), machine sessions, enrollment, matcher/outbox/pubsub services, diagnostics | 48 code files, ~13.8k LOC (75 files incl. migrations at first measure) | ~69 / 45 / 16 / 18 model classes (`runner/models.py`: `AgentRun`, `Runner`, `Pod`, `DevMachine`, `MachineToken`, sessions…) | 11 tasks / 1 (`runner/signals.py`) / `tests/contract/runner`, `tests/unit/runner` | runner agents, desktop, web | Very high: `api`, `authentication`, `cloud_agent`, `core`, `db`, `managed_runner`, `settings` (redis) | **High** — realtime paths; Channels consumer already a reject-stub (`runner/consumers.py`), so the WS replacement is greenfield |
| `orchestration` ↻ | Dispatch engine: scheduling, state transitions, workpad, agent phases, signals (no routes) | 10 files, ~3.7k LOC (was 8 / ~3.0k; `wake.py`, `workpad.py` added) | 1 (noise) / 0 / 0 / 0 own models | 0 tasks / `orchestration/signals.py` (+ `_DISPATCH_IMMEDIATE_ATTR`, `MOVED_BY_RUN_ATTR` flags) / `tests/unit/orchestration` | internal (runner, bgtasks, assistant call in) | High: `core`, `db`, `prompting`, `runner`, `utils` | **High** — signal side effects; needs dual-run care |
| `assistant` ↻ | AI assistant: threads/turns, SSE event stream, tool calls (issues, github, runs), MCP servers, BYOK llm configs (decrypt via `license`) | 37 files, code ~4.7k LOC | ~12 / 12 / ~3 + per-view / 6 model classes (`assistant/models.py`: threads, turns, events, MCP servers, llm configs) | 2 tasks (turn run, stale-turn sweep) / 0; `tests/contract/assistant` | web | High: `app`, `core`, `db`, `ee` (model provider), `managed_runner`, `search`, `settings` (redis), `license`, `utils` | **High** — streaming + secrets + spending brake (`assistant_message` 30/hour throttle) |
| `prompting` ↻ | Prompt templates: sections, overrides, compiled/kind rendering, seed + reseed commands; **plus `fragments/` + `sections/` markdown** (prompt-content system: intro, relationships, session framing, pidash-cli, posture, autonomy, state routing, workpad template) | 21 py files, ~3.4k LOC + markdown content dirs (was 27 files incl. migrations / ~3.1k) | ~4 / 4 / ~2 / 2 (`PromptTemplate`, `PromptSectionOverride`) | 0 / 0; `tests/contract/prompting`, `tests/unit/prompting` (grew: composer/context tests) | web/admin (template admin), internal (orchestration/assistant compose) | Medium: `db`, `runner` at import level (orchestration/assistant compose at runtime) | Medium — the markdown content ships as data with the slice, not code |
| `scheduler` | Builtin scheduler seeding on workspace create (`scheduler/signals.py`, `scheduler/builtins`) | 3 files, 235 LOC | 0 / 0 / 0 / `Scheduler`, `SchedulerBinding` live in `db` | 0 / 1 `post_save(Workspace)` seeder / `tests/unit/scheduler` | internal; surfaced via `app/urls/scheduler.py` (web) | Low: `db` only | Low — but the `post_save` seeder must not double-fire during dual-run |
| `bgtasks` | Celery worker plane: ~40 task modules (mail, webhooks, sync, exports, notifications, scheduler, agent ticker, loop) + beat wiring consumers | 41 files, ~9.8k LOC | 1 (noise) / 0 / 0 / 0 | **60 of the 78 repo-wide task decorators** live here / `github_signals.py`, `scheduler.py` import signal receivers / `tests/unit/bg_tasks` | none directly (workers for all areas) | Very high: `api`, `app`, `db`, `integrations`, `license`, `loop`, `orchestration`, `runner`, `settings`, `utils` | **High** — broker/beat compatibility constrains the worker swap |
| `license` | Instance console (`api/instances/`): instance admin, configuration values, encryption helpers | 30 code files, ~1.9k LOC (37 files incl. migrations at first measure) | ~15 / 14 / 7 / `Instance`, `InstanceAdmin` (`license/models`) | 1 / 0 / 0 | admin | Low-Medium: `app` (shared helpers), `authentication`, `config`, `db`, `utils` | **Low** — pilot candidate |
| `analytics` | Empty: `__init__.py` + `apps.py` only (13 LOC, no routes/views/models) | 2 files, 13 LOC | 0 / 0 / 0 / 0 | 0 / 0 / `tests/unit/analytics` (orphan — verify) | none | None | None — **skip** (disposition `skip`, §6) |
| `search` | FTS library (`search/issue.py`): `SearchVector`s pinned byte-identical to `issues_fts_idx` / `issue_comments_fts_idx` (pins verified at re-measure); no routes | 2 files, 237 LOC | 0 / 0 / 0 / 0 | 0 / 0 / `tests/unit/search` | web via `app`/`api` search endpoints | Low: `db` only | Low — port as library with index-parity test |
| `integrations` | Git provider adapters (github/gitlab) + code-review helpers; no routes | 9 files, ~1.6k LOC | 0 / 0 / 0 / 0 | 0 / 0 / `tests/unit/integrations` | internal (`app` integration views, `bgtasks` sync tasks) | Low: `db`, `license` (tokens), `utils` | Low-Medium |
| `cloud_agent` | Cloud dispatch: admission, creation, policy, events, github MCP; tasks | 15 files, ~1.7k LOC | 0 / 0 / 0 / 0 own models | 3 named tasks (`cloud_agent.scan_queued_runs`, `cloud_agent.sweep_stale_runs`, …) / 0 | internal (orchestration/runner) | Medium: `assistant`, `core`, `db`, `runner` | Medium |
| `managed_runner` | Desktop/managed execution: policy, admission, permissions (`IsDesktopSession`), tasks | 6 files, 290 LOC | 0 / 0 / 0 / 0 | 1 (`managed_runner.expire_waiting_runs`) / 0 / `tests/unit/managed_runner` | desktop | Medium: `core`, `runner` (↔ cycle) | Medium |
| `loop` | Auto-PM jobs (`users/me/auto-pm/…`, `loop/urls.py`, `loop/views.py`) | 9 files, 764 LOC | ~5 / 5 / ~1 / `LoopJob`, `LoopTarget`, `LoopUserPreference` in `db` | 0 (served by `bgtasks/loop.py`) / 0; `tests/contract/loop` | web | Medium: `app`, `assistant`, `bgtasks`, `db`, `license` | Low |
| `ee` ↻ | OSS extension stubs (no routes): `ee/assistant/{model_provider,stt_provider}.py`, `ee/cloud_agent/{toolsets,model_provider}.py`, `ee/authentication/desktop.py`, `ee/settings/user_settings.py` | 11 files, 367 LOC (was 10 / 295; `stt_provider.py` added) | 0 / 0 / 0 / 0 | 0 / 0 / 0 | private overlay (via stubs only — not detailed here) | Imported by `assistant`, `cloud_agent`, `core` | Low — keep stub-for-stub; the Rust backend needs equivalent injection points as traits with defaults (see §4) |
| `db` ↻ | Schema owner: ~30 model modules (`db/models/`), **223 migration files (55 with `RunPython`)**, ~20 management commands (`db/management/commands/`), mixins, read-replica routing | 60 code files, ~7.6k LOC (235 files incl. migrations at first measure) | 1 (noise) / 0 / 0 / **164 class defs in model files** (~165 concrete models incl. `Issue`, `Project`, `Workspace`, `User`, `State`, `Cycle`, `Module`, `Page`, `Scheduler*`, `Webhook`, `APIToken`, `Session`…) | 0 tasks / `db/models/user.py` receivers / `tests/unit/models` | ops (migrations, commands) | Hub: everything reads `db`; `db` itself imports `bgtasks`, `core`, `license`, `utils` (signals/tasks — the cycles) | **Structural** — exactly one side owns the schema at any time (parent rule); Rust starts read/write against the same tables, never owns migrations until cutover |
| `middleware` | `middleware/{db_routing,logger,request_body_size}.py` + `SessionMiddleware`; enabled list in `settings/common.py` `MIDDLEWARE` | 5 files, 362 LOC | 0 / 0 / 0 / 0 | 0 / 0 / `tests/unit/middleware` | all | Low: `bgtasks`, `utils` | Low — port as axum/tower middleware |
| `utils` ↻ | Shared kit, 73 files, ~12.3k LOC (was 72 / ~11.5k; `openapi/` grew): paginators (`BasePaginator`, `GroupedOffsetPaginator`), filters (`ComplexFilterBackend`, `IssueFilterSet`), `issue_filters`, `order_queryset`, cache, host/URL, timezone, CSV, porters, github clients, duplicated permission trees (`utils/permissions/` mirrors `app/permissions/` — both trees verified present at re-measure) | 73 files, ~12.3k LOC | ~4 (noise) / 0 / 1 / 0 | 0 / 0 / `tests/unit/{serializers,permissions,utils}` | all | High fan-out: `app`, `bgtasks`, `db`, `license`, `runner` (↔ cycle with `app`) | Medium — port once as the shared kernel; deduplicate the twin permission trees |
| `core` | 5 files, 478 LOC: `agent_execution.py` (executor routing), `permissions.py`, `querysets.py`, `user_settings.py` | — | 0 / 0 / 0 / 0 | 0 / 0 / 0 | internal | `db` at import level (`ee`, `managed_runner`, `runner` compose at runtime) | Medium — executor routing is load-bearing for 6/8/9 |
| `config` | 3 files, 496 LOC: `accessor.py` + `registry.py` (settings overlay mechanism) | — | 0 / 0 / 0 / 0 | 0 / 0 / `tests/unit/config` | overlay | `license` (import-level; wider use at runtime) | Low — keep semantics identical (see §4) |
| `settings` | 9 files, ~1.8k LOC: `common/local/production/test/mongo/openapi/redis/storage` + celery beat schedule (`celery.py`: `beat_schedule`, `DatabaseScheduler`) | — | 0 / 0 / 0 / 0 | beat: `celery.py` `beat_schedule` / 0 / `tests/unit/settings` | ops | `config`, `utils` | Low — overlay-compatible settings are a migration prerequisite |
| `throttles` | `throttles/asset.py` (15 LOC; `AssetRateThrottle`, 5/min) | — | 0 / 0 / 0 / 0 | 0 / 0 / 0 | asset downloads | None | Minimal |

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
reuses the slice-0 kernel instead of forking helpers.
`Disposition` is the Stage 0 verdict, recorded here (not in a TSV): `full` = port everything in
the slice; `minimal` = port narrowed behaviour only; `skip` = never filed, excluded from
contract-test scope. The filer (§9) reads this column.

| # | Slice (prefix / area) | Ships | Disposition | Why this position |
|---|---|---|---|---|
| 0 | Scaffold + `web/` edge | axum app, router-by-prefix proxy, kernel ports (`utils` paginators/filters/timezone, middleware equivalents, auth *library* incl. session/token/machine validation, permission unification), settings/extension-point equivalents, parity harness; `web/` (2 routes, no DB) proves routing + health-gated deploy | `full` | Unblocks everything; zero product risk; proves the rollback path (routing change) on day one |
| 1 | `license` — `api/instances/` (**first pilot**, see §6b) | 15 routes, `Instance*` models, `register/configure_instance` commands | `full` | Small vertical that exercises DB R/W + auth + config overlay end to end |
| 2 | `space` — `api/public/` | 25 read-mostly routes, S3 asset serving | `full` | Isolated client; proves public/weak-auth path; no core-model writes (verify per endpoint) |
| 3 | `loop` — auto-pm endpoints | 5 routes + `bgtasks/loop.py` worker path | `full` | Small; exercises `assistant`/`bgtasks` coupling in miniature before those slices |
| 4 | `prompting` + `scheduler` | 4 routes, prompt content (`fragments/`, `sections/` as data, not code), seed/reseed commands, `post_save(Workspace)` seeder | `full` | Pure service; settle the signal-ownership rule (Django keeps seeder until cutover) while blast radius is tiny |
| 5 | `integrations` (library) + git sync task parity | adapters + `bgtasks/git*_task.py` | `full` | No routes; must precede `app`/`api` slices that render linked PR/review data |
| 6 | `assistant` — `api/…/ai-assistant/` + MCP + SSE | 12 routes, threads/turns/MCP models, 2 tasks, BYOK decrypt, 30/hour brake | `full` | First stateful/LLM slice; needs slices 0–1 (redis, license-decrypt) and 5 (github tools) |
| 7 | `bgtasks` worker plane | task-by-task cutover, beat schedule port, broker compat | `full` | Workers must exist before engine slices move dispatch targets; per-task-group flags keep rollback routing-only |
| 8 | `cloud_agent` + `managed_runner` | admission/policy/dispatch, desktop session flow | `full` | Depends on 6 (assistant runs) and 7 (sweep/scan tasks) |
| 9 | `orchestration` (engine, no routes) | signals, state transitions, workpad, phase machine (incl. new `wake.py`/`workpad.py`) | `full` | Depends on 4, 7, 8; dual-run with one receiver-owner (Django until this slice cuts over) |
| 10 | `runner` — `api/v1/runner/`, `api/runners/`, realtime | 69 routes/views, 18 models, machine auth, WS/long-poll replacement (Channels side already a stub; replacement keeps close-code-1008 semantics) | `full` | Hardest realtime surface; goes after its dispatch/finalization dependencies (7–9) |
| 11 | `authentication` endpoints — `auth/` | sign-in/up, OAuth, magic, CSRF, CLI device flow, user-auth workflow | `full` | Blast radius is every client; the *mechanism* shipped in slice 0, endpoints move here once all consumers tolerate it |
| 12 | `api` — `api/v1/` external API | ~88 routes, token auth, schema docs | `full` | Third-party contract; needs the full parity harness and slices 5, 6, 10, 11 beneath it |
| 13 | `app` — `api/` core session API | ~264 routes, issue/search/asset/scheduler-occurrence hot paths (664 annotate sites, FTS parity) | `full` | Largest, hottest, most coupled — last product slice by design |
| 14 | Ops tail + cutover | 24 management commands, seeds data, admin, replica routing, Django/DRF removal from runtime image, promotion to permanent location | `full` | Only when every route is served by Rust (parent "Done when") |
| — | `analytics/`, `ws/runner/` stub, `runner/consumers.py`, `runner/routing.py`, `debug_toolbar` wiring | nothing ships | `skip` | Dead/retired/dev-only (§7); never filed, excluded from contract-test scope |

## 6b. Recommended pilot slices

**First pilot: slice 1 (`license`, `api/instances/`).**

- Small: ~1.9k LOC, 15 routes, 14 views, 7 serializers, 2 models — reviewable in one PR.
- Low risk: admin-only client; no issue/project hot paths; no signals, no realtime, no FTS.
- Real traffic: every self-host and cloud instance console hits it (instance check/configuration),
  so parity gaps show up immediately without endangering collaboration workflows.
- Few model dependencies: `Instance`, `InstanceAdmin` (+ shared `User` reads); small enough that the
  slice-0 kernel (auth library, permission unification, settings overlay, S3/redis access) gets
  exercised end to end before larger slices depend on it.
- Success bar: byte-equivalent responses on the contract replay for all 15 routes, dual-run with
  Django owning the two tables, rollback by routing change only.

**Second pilot: the issue-list domain end to end on the foundation crates** (per the general
design, stage 3). This pilot proves the foundation layer (types → db → services → api crate
graph), builds the layer fixture generators on its own rows, records generation + review time
per row, and drafts the porting guide from its own code. It needs the proxy drill only for its
cutover step and the Celery bridge only if its domain enqueues a task.

## 7. Delete rather than port (disposition `skip`)

- `analytics/` — empty app (13 LOC, no routes/views/models/signals). Delete; remove from
  `INSTALLED_APPS`. Verify the orphan `tests/unit/analytics` dir goes with it.
- `ws/runner/` + `runner/consumers.py` + `runner/routing.py` — control-plane WebSocket already
  retired to a reject-stub; remove instead of porting. (The *replacement* realtime path is designed
  in slice 10, which preserves the close-code-1008 behavior old runners rely on, not here.)
- `debug_toolbar` wiring (`urls.py` `DEBUG` branch, `settings/local.py`) — dev-only; do not port
  (re-add natively in Rust dev tooling if wanted).
- Migrations history (223 files) is **not** ported by any slice: Django keeps owning the schema
  until the slice-14 cutover, per the one-schema-owner rule.
- Confirm-with-product before porting (candidates, not decisions): `ExporterHistory`/`Importer`
  porters (`app/urls/{exporter}.py`, `bgtasks/export_task.py`, `utils/porters/`) — live but
  low-traffic; either a late small slice or removal. The legacy non-FTS search contract
  (`include_comments=False` callers in `app`) — keep behavior, but do not carry the legacy flag
  forward longer than slice 13.

## 8. Per-slice parity, testing, and rollback (applies to follow-up issues)

- Parity: replay the area's `tests/contract/*` + `tests/unit/*` cases against the Rust slice
  through the proxy router; FTS slices must assert identical `EXPLAIN` index usage
  (`issues_fts_idx`); throttle scopes and CSRF/session semantics are asserted per route, not assumed.
- Rollback of any slice is a routing change back to Django (no redeploy of old code), with both
  backends on the same Postgres/Redis and Django owning the schema until slice 14.
- Each follow-up sub-issue follows the porting guide and answers to layer fixtures; the domain
  gate answers to the contract tests (general design, rule 4). Sub-issues do not write their own
  designs. Filing mechanics are §9.

## 9. How conversion issues are filed from this document

This section is the filer’s contract. It replaces the generation-script-from-TSV model: issues
are created by reading the tables above — by a human or an agent run filing through the
`pidash` CLI — never from a generated spreadsheet.

- **Slice → domain epic.** Each numbered slice in §6 becomes one domain epic per filing stage
  (stages 4–5; stages 0–3 are single issues, see the general design). The epic carries: the slice
  number and name, the `Ported from` Django commit sha (this is the drift record — a re-sync is
  filed when `apps/api` moves past it), the Python source paths from §1, and the disposition.
  Rows with disposition `skip` are never filed.
- **Epic → sub-issues.** Within a domain epic, work is ordered bottom-up through atomic layers:
  serializers, models, queries, permissions/throttles, tasks — composed into thin handlers and
  shipped as one vertical slice. Decomposition happens at filing time, never in runs: the first
  sub-issue of every domain epic is its **fixture** sub-issue (runs the layer generators over the
  domain's rows; commits fixtures with trace tables and EXPLAIN checks); every layer sub-issue is
  `blocked_by` it; each sub-issue carries at most 5 rows or one topological closure, sized so one
  agent run finishes it end to end; the **domain gate** is filed last and runs the domain's named
  contract tests on both backends. `minimal` rows are narrowed at filing (fewer sub-issues, same
  shape).
- **Relationships.** Filed with `pidash issue relate`: parent/child for epic → sub-issue, sibling
  for sub-issues of the same layer, `blocked_by` for order. The `Depends on` body field mirrors
  the edges for human readers; the edges are what the machine reads.
- **Self-containment (rule 8).** Every sub-issue carries: domain name, `Ported from` commit,
  Python sources, the only Rust paths it may touch, its fixture ids, its contract tests, reference
  pages with `updated_at`, and exact done criteria — so a fresh agent executes it cold whenever
  the Release scheduler moves it to In Progress. Nothing is filed until its content prerequisites
  are green (binding wiki pages agent-readable, proven pilot loop, generators proven on the pilot).
- **Re-syncs without a TSV.** When Django moves past a domain epic's `Ported from` commit, the
  Drift scheduler diffs `apps/api` since that commit and files a fixture-regeneration issue plus a
  re-sync sub-issue under the domain epic, `blocked_by` the regeneration. `Ported from` advances
  only when the re-sync reaches Done. Migrations landing after the recorded commit file a re-sync
  automatically (Django owns the schema throughout).

## Open questions for Rich

1. Pilot choice: `license` first (recommended, §6b), then the issue-list foundation pilot per the
   general design — confirm, or swap `space` (more traffic, read-only) into the first slot?
2. May slice 0 unify the twin permission trees (`app/permissions/` vs `utils/permissions/`) into one
   kernel module, or must both stay importable during dual-run?
3. Are the exporter/importer porters still supported product surface, or delete candidates?
