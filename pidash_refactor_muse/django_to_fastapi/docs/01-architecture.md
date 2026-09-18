# FastAPI backend: target architecture, stack, and project layout

Status: **draft for review** (PIDASHCONV-3). Design only — no code.
Public document: describes OSS structure and generic extension points only,
never private overlay internals.

This document decides what the new backend looks like before any of it is
written. It builds on the inventory in `00-inventory-and-slices.md`
(PIDASHCONV-2, under review at the time of writing — figures quoted below are
re-measured or cited from that draft where noted). The database layer has its
own sub-issue; this document states only the boundary the DB design must
satisfy (§11) and does not duplicate it.

Parent rules (PIDASHCONV-1) apply throughout: nothing outside
`pidash_refactor_muse/` imports from it; the directory stays out of the pnpm
workspace, Docker images and build contexts, compose files, and release
workflow until an approved design says otherwise; PRs touch only files under
`pidash_refactor_muse/`; no `apps/api` behavior, schema, or migration changes.

## 1. How the Django code is layered today

Source: `apps/api/pi_dash` at `main` (`ce3cf2ad`). Numbers below are measured,
not impressions.

- **Views (HTTP layer).** ~396 view classes / ~529 routes. Two parallel REST
  surfaces: `app/` (session-auth, `api/` prefix, 22 URL modules, ~184 view
  classes) and `api/` (token-auth, `api/v1/` prefix, 16 modules, ~56 view
  classes), plus `space/` (public reads), `authentication/` (~43 views:
  sessions, OAuth, device flow), `runner/` (~45 views), `assistant/` (SSE
  streams), `license/`, `loop/`, `prompting/`, and the `web/` edge
  (`robots.txt`, health check). Views are class-based DRF views/viewsets.
- **Serializers (validation + representation).** 257 serializer classes.
  Serializers do double duty: input validation *and* output shaping, often with
  per-view subclasses and method fields that hide queries.
- **Models (persistence).** 165 models / 219 migrations. Nearly all state lives
  in `db/models/`; four out-of-`db` homes exist (`runner/models.py`: 18 model
  classes, `assistant/models.py`: 6, `prompting`: 2, `license/models`).
  Custom managers (`SoftDeletionManager`, `IssueManager`, `StateManager` in
  `db/models/{issue,state}.py`) and `BaseModel.save()` auto-stamping
  `created_by/updated_by` via django-crum (37 refs) put behavior inside the
  ORM layer.
- **Utils (shared kernel).** 72 files, ~11.5k LOC: custom cursor paginators
  (`utils/paginator.py`, `utils/global_paginator.py` — no DRF built-in
  pagination in use), `ComplexFilterBackend` / `IssueFilterSet`
  (`utils/filters/` package: `filter_backend.py`, `filterset.py`),
  `issue_filters`, `order_queryset`, cache, host/URL,
  timezone, CSV, porters, GitHub clients, and a full permission tree
  (`utils/permissions/`) that **mirrors** `app/permissions/` (~1,028 lines
  combined across the two trees).
- **Bgtasks (worker plane).** 41 files, ~9.8k LOC; 60 of the 78 repo-wide
  Celery task decorators live here (mail, webhooks, sync, exports,
  notifications, scheduler, agent ticker, loop), with beat wiring in
  `pi_dash/celery.py` (`beat_schedule`, `DatabaseScheduler` via
  django-celery-beat). `bgtasks/{github_signals,scheduler}.py` bridge events
  into tasks.
- **Cross-cutting.** 168 `transaction.atomic` sites; ~38 signal receivers across
  10 files (`scheduler`, `orchestration`, `runner`, `db/models/user.py`,
  `bgtasks`); DRF throttles (`AnonRateThrottle` 30/min default; per-scope rates
  in `settings/common.py`: `user` 120/min, `asset_id` 5/min, `runner_chat_send`
  60/min, `auth_device_start` 20/min, `assistant_message` 30/hour,
  `assistant_llm_test` 6/min); OpenAPI via drf-spectacular
  (`settings/openapi.py`, title "The Pi Dash REST API"); custom session auth
  (`Session` model + `DBSessionStore` + `SessionMiddleware`, CSRF via
  `CsrfViewMiddleware` + `auth/get-csrf-token/`).

### What makes this slow for agents to change

1. **Fat views.** `apps/api/pi_dash/api/views/issue.py` is 2,975 lines;
   `app/views/issue/base.py` 1,489; `app/views/integration/github.py` 1,205.
   Queryset construction, permission checks, serialization tweaks, and side
   effects (tasks, signals) live in the same class body, so a one-endpoint
   change requires reading thousands of lines and risks touching neighbors.
2. **Hidden signal side effects.** Creating a `Workspace` fires
   `scheduler/signals.py` (seeds builtin schedulers); agent dispatch hides in
   `orchestration/signals.py` (with re-entrancy guards
   `_DISPATCH_IMMEDIATE_ATTR`, `MOVED_BY_RUN_ATTR`); run finalization hides in
   `runner/signals.py` + model receivers. Behavior is invisible at the call
   site — the exact failure mode agents cannot see.
3. **Deep inheritance and twin hierarchies.** View inheritance chains plus
   *two* parallel permission trees (`app/permissions/` vs
   `utils/permissions/`, same class names) plus `core/permissions.py`,
   `runner/services/permissions.py`, `managed_runner/permissions.py`, and
   `license/api/permissions/instance.py`. An agent fixing auth must first
   discover which of six trees actually guards the route.
4. **Import cycles that forbid bottom-up reasoning.**
   `db ↔ bgtasks/orchestration/runner`, `app ↔ api`, `orchestration ↔
   prompting`, `assistant ↔ cloud_agent/ee`, `core ↔ ee/managed_runner/runner`
   (measured from `^from pi_dash.<area>` imports, migrations excluded). No
   module can be understood in isolation.
5. **Serializers as hidden query layers.** 257 serializers with method fields
   and per-view subclasses mean N+1s and business rules hide in the
   "representation" layer, and the 662 `.annotate(` / 114 `Subquery` / 240
   `OuterRef` sites concentrate in paths agents must touch for listing/search.

Design consequence: the new backend must make the request path **linear and
explicit** — router → service → repository — with no signals, no inheritance
for reuse, and exactly one permission-check location per route.

## 2. Repo conventions the new directory must respect

Measured from `main`:

- **pnpm + turbo workspace** (`pnpm-workspace.yaml`, `turbo.json`, `AGENTS.md`).
  Workspace packages are `apps/*` and `packages/*`, minus `apps/api` and
  `apps/proxy`. A top-level `pidash_refactor_muse/` directory matches no
  workspace glob, so it is **already outside** the workspace; the scaffold
  must add an explicit exclusion comment/guard so a future `apps/*`-style
  widening cannot sweep it in, and must not add a `package.json` that turbo
  could pick up.
- **Root `.dockerignore`.** The web/admin/space/live images build **from the
  repo root** (`docker-compose.yml`: `context: .`, e.g.
  `apps/web/Dockerfile.web` runs `COPY . .` in its builder stage). The root
  `.dockerignore` is therefore the mechanism that keeps the new directory out
  of those images. The api image builds with `context: ./apps/api` and is
  unaffected either way. Recommendation: add an explicit
  `pidash_refactor_muse/` entry to the root `.dockerignore` at scaffold time
  (belt-and-braces alongside the workspace non-membership).
- **Copyright header check** (`COPYRIGHT_CHECK.md`). Every tracked `*.py`
  file (except `**/migrations/**`) must carry the `COPYRIGHT.txt` header
  (`addlicense --check`). The scaffold must apply headers from day one and
  wire the check into the new directory's CI (see §3, lint row).
- **`AGENTS.md`.** Day-to-day commands (`pnpm dev/build/check`), oxfmt +
  OxLint, strict TypeScript, MobX patterns. It governs the JS workspace only;
  the backend gets its own agent-facing conventions in §9, written to the same
  audience.

## 3. Stack decisions

| Decision | Options considered | Recommendation |
|---|---|---|
| Python version | 3.11 (widest image support) / **3.12** / 3.13 (newest) | **3.12.** Matches the current runtime (`apps/api/Dockerfile.api`: `python:3.12.10-alpine`), so behavior-affecting stdlib differences are eliminated during dual-run. `fastmcp` and `pydantic-ai` are already dependencies, both 3.12-clean. Revisit 3.13 only after cutover. |
| Dependency manager | pip+`requirements/*.txt` (status quo) / Poetry / **uv** | **uv** with `pyproject.toml` + `uv.lock`. pip gives no lockfile (unreproducible agent environments); Poetry is slower and its resolver fights the scientific-image edge cases this repo hits (`Dockerfile.api` installs with cargo/gcc/postgres-dev). uv produces a lockfile, is an order of magnitude faster, and understands `pyproject.toml` natively. |
| Lint + format | ruff (status quo) / flake8+black | **ruff (lint + format)**, same config family as `apps/api/pyproject.toml` (`line-length = 120`, `E`+`F`, isort `known-first-party`). One tool, same muscle memory, and the scaffold copies the config so style drift is impossible. |
| Type checking | none (status quo — no mypy/pyright config today) / mypy / pyright | **mypy in strict mode**, run in CI. Rationale: agents produce more correct code under a strict checker than under none, and strict-from-day-one is cheap on greenfield while strict-later is prohibitive. pyright is a close second; mypy wins on unix-socket-free CI simplicity and `.ini`-free `pyproject` config. Pydantic v2 plugins for mypy are enabled. |
| Test runner | pytest (status quo) / unittest | **pytest**, mirroring `apps/api/pytest.ini` markers (`unit`, `contract`, `smoke`, `slow`) so slice parity suites slot into familiar lanes. HTTP tests use FastAPI `TestClient` (httpx-based); async tests via `anyio`/`pytest-asyncio` (one choice, fixed at scaffold). Contract tests from `apps/api/pi_dash/tests/contract/` are the parity oracle (see §11 of the parent rule 2 pattern: options, risks, parity strategy, rollback). |

## 4. Package layout and the business-logic rule

```
pidash_refactor_muse/django_to_fastapi/backend/
  pyproject.toml            # uv project; ruff/mypy/pytest config lives here
  uv.lock                   # reproducible installs (committed)
  README.md                 # how to run, test, lint (agent entry point)
  src/
    pidash_api/             # one importable package; nothing else imports it
      __init__.py
      main.py               # create_app() factory only; no import-time side effects
      deps.py               # shared FastAPI dependencies (auth, request id, db session)
      core/                 # cross-cutting: config, logging, errors, pagination,
                            # filtering helpers, permissions base, middleware
        config.py
        logging.py
        errors.py           # error envelope + exception handlers (§6)
        pagination.py       # DRF-compatible cursor pagination (§6)
        filtering.py        # explicit filter parsing (§6)
        permissions.py      # THE single permission-check location per route
        middleware.py       # request-id, access log, body-size limit
      db/                   # BOUNDARY — owned by the DB-layer sub-issue (§11).
                            # Repositories depend on interfaces declared here;
                            # engines/sessions/models arrive with that design.
        __init__.py         # (placeholder; real content per DB design)
      modules/              # one bounded area per slice (mirrors migration slices)
        health/             # first module: robots.txt + health check (port of `web/`)
          __init__.py
          router.py         # thin: parse/auth → service → map errors
          service.py        # business logic lives HERE and only here
          repository.py     # persistence lives HERE and only here
          schemas.py        # Pydantic I/O models (§6)
          permissions.py    # area checks, composed from core.permissions
        <area>/             # license/, loop/, … one directory per migrated slice
          router.py
          service.py
          repository.py
          schemas.py
          permissions.py
      worker/               # Celery task wrappers (broker-compatible; §7)
        __init__.py
        tasks.py
  tests/
    unit/                   # service/repository logic, no HTTP
    contract/               # HTTP parity vs DRF responses (§6, §11)
    smoke/                  # boot + health + auth handshake
```

**The rule for where business logic lives (non-negotiable):**

- **Routers** do four things only: parse/validate input (schemas), enforce
  auth + permissions (via `deps.py`), call exactly one service method, map
  service errors to HTTP. No `if` that encodes a business rule; no queries.
- **Services** own every business rule, transaction boundary, and side effect
  (task enqueue, events). They take plain inputs, return plain outputs, and
  never import FastAPI, HTTP, or the ORM session factory directly —
  dependencies arrive as constructor arguments (explicit DI, §9).
- **Repositories** own all persistence access (queries, ordering, pagination
  application, FTS vectors). Services never write query code; routers never
  touch repositories.
- **Schemas** own wire shape only (Pydantic models). No business methods, no
  DB access — the DRF method-field N+1 trap (§1.5) is banned by construction.
- `core/` owns anything used by two or more modules (config, errors,
  pagination, permission base). A third copy of anything triggers extraction
  into `core/`; slices must not fork a third permission tree (§1.3).

## 5. Schemas, errors, pagination, filtering, OpenAPI

Compatibility principle: **API behavior is a contract** (parent Constraint).
Every shape below must stay byte-compatible with what clients receive from
DRF today; contract tests pin it.

- **Pydantic v2** for all I/O models (`BaseModel`, `ConfigDict(populate_by_name=True)`).
  Request/response pairs are separate classes (`<X>In` / `<X>Out`) so input
  leniency never leaks into output shape. Field names match DRF serializer
  output keys exactly, including existing camelCase where DRF emits it —
  compatibility beats style purity.
- **Error format.** DRF's default is `{"detail": ...}`; the custom
  `auth_exception_handler` (`authentication/adapter/exception.py`) adds
  `error_code`/`error_message` dicts for auth failures and maps throttles to
  429. Recommendation: one envelope — `{"detail", "code"?, "errors"?}` —
  with exception handlers that reproduce both shapes: generic errors emit
  DRF-style `{"detail"}`; auth/permission/throttle errors emit the existing
  `error_code` dicts verbatim. Status codes mirror DRF exactly (401 for
  `NotAuthenticated`, 429 for throttles, 403/404 semantics unchanged).
- **Pagination.** DRF built-ins are not in use; the custom cursor paginator
  (`utils/paginator.py`: `Cursor` `value:offset:is_prev`, `CursorResult`
  `results/next/prev/hits/max_hits`) is. Recommendation: port the paginator
  as a pure, dependency-free `core/pagination.py` with byte-identical cursor
  encoding and response keys, pinned by contract tests against recorded DRF
  responses. Offset/limit styles stay per-endpoint as today — no
  normalization during migration.
- **Filtering.** `ComplexFilterBackend` / `IssueFilterSet` semantics
  (`utils/filters/` package, `utils/issue_filters.py`, `utils/order_queryset.py`)
  are re-implemented as explicit, typed filter parsers in
  `core/filtering.py` + per-module filter models. No `django-filter`
  dependency in the new backend; every accepted query param is declared on
  the schema (agents can see the full surface in one file).
- **OpenAPI generation.** FastAPI's native generation replaces
  drf-spectacular. Title/version/contact mirror `settings/openapi.py`
  ("The Pi Dash REST API"); schema served from the same paths clients use
  today. The pilot slice must diff the generated schema against the DRF one
  for its routes and explain every delta.

## 6. Sync or async

- **Recommendation: `async def` endpoints by default**, served by uvicorn
  (the current stack already runs uvicorn workers under gunicorn on ASGI, so
  operations keeps its playbook).
- **Blocking work never runs on the event loop.** ORM calls, CPU-bound
  composition, and subprocess/ffmpeg-style work go through
  `anyio.to_thread.run_sync` (or a bounded worker pool fixed at scaffold)
  until the DB-layer design lands; that design owns the final sync/async
  session story and this document defers to it.
- **Celery stays the worker plane.** 60 task modules + beat
  (`DatabaseScheduler`) cannot move without broker/beat compatibility work;
  FastAPI enqueues the same tasks with the same names/args (dual-run safe).
  Replacing Celery is out of scope for the migration.
- **Streaming (assistant SSE) and the retired Channels stub** (`runner/`
  consumer already rejects all traffic): greenfield async generators in the
  owning slice, not part of the scaffold.

## 7. Settings and configuration

- `pydantic-settings` `Settings` class in `core/config.py`, one field per
  environment variable.
- **Reuse existing environment variable names verbatim** — every name in
  `apps/api/pi_dash/settings/*.py` (`common/local/production/test/redis/
  storage/mongo/openapi`) keeps its spelling and semantics (e.g. `AWS_*`,
  `CELERY_*`, `DEBUG`, `ALLOWED_HOSTS`, `DATABASE_URL`-family,
  `ASSISTANT_*`, `CLOUD_AGENT_*`). The scaffold ships a name-mapping table
  generated from the settings modules; any rename is a breaking change and
  is forbidden without an approved design.
- Overlay compatibility (private overlay composes via Django settings overlay
  + file overlay today): the FastAPI backend exposes equivalent extension
  points — a documented `Settings` subclass hook and a plugin-module import
  list (generic mechanism only; no private internals in this repo, Rule 3).
  Exact overlay mechanics are detailed in the slice that migrates `config`/
  `settings` semantics, not here.

## 8. Logging, OpenTelemetry, request IDs

Match what Django emits today so dashboards and alerts keep working:

- **Request IDs.** Middleware generates (`uuid4`) or propagates
  (`X-Request-ID` inbound) a request ID, stores it on
  `request.state` + `contextvars`, echoes it back on every response, and
  includes it in every log record and error payload.
- **Logging.** Structured JSON access + application logs with the same fields
  operators query today (timestamp, level, logger, request_id, method, path,
  status, latency). Port `middleware/logger.py` and `middleware/db_routing.py`
  semantics as ASGI middleware in `core/middleware.py`; keep the Django
  body-size limit (`middleware/request_body_size.py`, `DATA_UPLOAD_MAX_…
  ` family) identical.
- **OpenTelemetry.** SDK spans per request with the same service naming the
  Django app uses; trace/span IDs join the log records. No new collector
  topology — the backend emits to the existing pipeline.

## 9. Conventions that keep the code easy for agents

1. **Small modules.** Soft cap ~300 lines per module; anything bigger is a
   review finding. (Contrast §1.1: 2,975-line views.)
2. **Explicit dependencies.** Constructor injection everywhere; `deps.py`
   wires FastAPI `Depends`. No service locators, no globals, no
   `lru_cache`-hidden clients.
3. **No import-time side effects.** Importing any module must not connect,
   migrate, spawn, register signals, or read the network. App construction
   happens in `create_app()`; startup work lives in lifespan handlers.
   (This is the anti-signal rule, §1.2.)
4. **Composition over inheritance.** No view/service base-class hierarchies;
   shared behavior is a function in `core/` or a declared dependency.
   (Anti-§1.3.)
5. **One permission location.** Each route declares its checks in its
   module's `permissions.py`, composed from `core.permissions`. Six-tree
   archaeology ends here.
6. **Typed boundaries.** `mypy --strict` clean on every PR; public service
   signatures fully annotated so agents can use a module without reading its
   body.
7. **README as entry point.** `backend/README.md` documents run/test/lint in
   copy-paste form; every module directory gets a one-paragraph `__init__`
   docstring stating what lives there.

## 10. Isolation: staying invisible to the running product

- **Own project file + lockfile.** `backend/pyproject.toml` + `uv.lock`,
  disjoint from `apps/api/requirements*` and `apps/api/pyproject.toml`. No
  shared virtualenv, no shared dependency set.
- **Out of the pnpm workspace.** Already outside it by path (§2); scaffold
  adds an explicit guard comment in `pnpm-workspace.yaml`'s vicinity
  (without changing workspace membership) — actually implemented as: no
  `package.json` under `pidash_refactor_muse/`, so pnpm/turbo have nothing to
  claim. If a future change widens workspace globs, that change's design must
  re-exclude this directory.
- **Out of Docker build contexts.** Add `pidash_refactor_muse/` to the root
  `.dockerignore` at scaffold time (protects the root-context web/admin/
  space/live images); the api image (`context: ./apps/api`) never sees it.
  No Dockerfile, compose file, or release workflow references the directory
  until an approved design says so.
- **No imports in either direction.** Nothing outside `pidash_refactor_muse/`
  imports from it (parent Rule 1), and — equally — it imports nothing from
  `apps/api`. Rationale: importing `pi_dash.*` pulls Django settings, app
  registry, and signal wiring (import-time side effects, §1.2) into the new
  process; there is no safe subset.
- **Read-only reuse: disallowed as imports, allowed as ported copies.**
  Pure logic with no Django imports (cursor-codec semantics in
  `utils/paginator.py`, FTS vector definitions in `search/issue.py` pinned to
  `issues_fts_idx` / `issue_comments_fts_idx`, throttle rates) may be
  **duplicated** into the new tree with a `# Ported from <path>@<sha>`
  pointer and a parity test. Copies, never imports; the pointer + test is
  what keeps them honest when the original moves.

## 11. Promotion and the database boundary

- **Database layer: separate sub-issue, referenced not duplicated.** This
  document fixes only the boundary: repositories depend on session/engine
  interfaces declared in `backend/src/pidash_api/db/`; Django owns the schema
  and all migrations until cutover; both backends share one Postgres + one
  Redis during migration with exactly one schema owner at any time (parent
  Constraint). Model definitions, migration strategy, replica routing, and
  the sync/async session decision belong to the DB design.
- **Promotion.** Once every route is served by FastAPI (parent Done
  criteria), the service moves from
  `pidash_refactor_muse/django_to_fastapi/backend/` to its permanent home —
  **recommended: in-place replacement of `apps/api`** — via `git mv`, which
  preserves file history. The `pidash_api` package name is kept or renamed in
  the cutover design; that design also removes Django/DRF from the runtime
  image and re-homes CI (today's API lint/test workflows trigger only on
  `apps/api/` changes, so the scaffold ships its own workflow scoped to the
  new directory, off by default for everyone else per Rule 1).
- **Rollback per slice** (parent Constraint): every migrated slice keeps a
  routing-level rollback (proxy/reverse-proxy switch back to Django), never a
  redeploy of old code. Slice designs detail their switches; this document
  requires only that the layout keeps slices independently routable
  (one module ⇒ one route prefix ⇒ one switch).

## 12. Risks

- Stale-port drift (§10 copies) — mitigated by source pointers + parity tests.
- Strict mypy slowing early slices — accepted; the review gate can relax to
  non-strict only with written justification.
- Schema-diff surprises in the pilot (DRF method fields with hidden queries)
  — mitigated by recorded-response contract tests before porting each route.
- `00-inventory` still under review — figures re-cited here were
  re-measured at `ce3cf2ad`; final alignment pass once both docs are
  approved.
