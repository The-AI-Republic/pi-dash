# 06 — Backend Architecture

The backend lives in `apps/api/` and is a **Django 4 / DRF / Channels** application. It is **not** in the pnpm workspace — separate venv, separate test runner, separate dependency manifest (`pyproject.toml` + `requirements.txt`).

## Top-level layout

```
apps/api/
├── manage.py
├── pyproject.toml                ← Python package metadata + ruff config
├── requirements.txt              ← pinned deps
├── pytest.ini                    ← --reuse-db --nomigrations
├── run_tests.py / run_tests.sh   ← test entrypoints (markers: unit | contract | smoke)
├── apple_pi_dash/                ← legacy Django project name (settings module path)
└── pi_dash/                      ← actual Django app modules (importable as `pi_dash.*`)
```

> Note: the Python package is named `pi_dash`, but `DJANGO_SETTINGS_MODULE` historically references `apple_pi_dash.settings.*` and you'll still see `apple_pi_dash` referenced in some files. They co-exist — don't rename them in a single PR.

## Django modules (`pi_dash/`)

```
app/             ← main product API (workspaces, projects, work items, cycles, modules, …)
api/             ← public v1 REST API (under /api/v1/)
authentication/  ← login, sign-up, OIDC, sessions
space/           ← published "spaces" — public/guest content (/api/public/)
license/         ← instance license verification (/api/instances/)
runner/          ← cloud side of the runner protocol — HTTP + WebSocket
orchestration/   ← AI agent run scheduling, phases, workpad (see 08)
prompting/       ← prompt composition, fragments, rendering (see 08)
scheduler/       ← built-in run/scheduling signals
analytics/       ← analytics endpoints, charts, exports
bgtasks/         ← Celery tasks
db/              ← shared DB utilities and mixins
logs/            ← request/audit logging
middleware/      ← throttling, CORS, auth, sentry hooks
throttles/       ← DRF throttle classes
seeds/           ← initial data, e.g. seed projects/labels
settings/        ← env-split settings (see below)
tests/           ← pytest test suite
utils/           ← shared helpers
web/             ← server-rendered/static catch-all
```

## URL tree

`pi_dash/urls.py`:

```
api/                    → pi_dash.app.urls          ← internal app API (consumed by web/admin/space)
api/public/             → pi_dash.space.urls        ← guest views of published content
api/instances/          → pi_dash.license.urls
api/runners/            → pi_dash.runner.web_urls   ← web UI calls for runner admin
api/v1/                 → pi_dash.api.urls          ← public versioned REST API
api/                    → pi_dash.prompting.urls    ← prompt templates
api/v1/runner/          → pi_dash.runner.urls       ← runner HTTP + WS endpoints
auth/                   → pi_dash.authentication.urls
/                       → pi_dash.web.urls          ← static / catch-all
```

OpenAPI/Swagger is exposed at `/api/schema/swagger-ui/` and `/api/schema/redoc/` when `ENABLE_DRF_SPECTACULAR` is true. Use that as the live endpoint reference — this wiki intentionally does not duplicate it.

## ASGI + Channels

`pi_dash/asgi.py` wires a `ProtocolTypeRouter`:

```python
application = ProtocolTypeRouter({
    "http": django_asgi_app,
    "websocket": URLRouter(runner_ws_urls),
})
```

- **HTTP** → standard Django view tree.
- **WebSocket** → routed by `pi_dash/runner/routing.py` to the runner consumers in `pi_dash/runner/consumers.py`.

The runner WebSocket path is the legacy v3 transport; the current wire protocol (v4) uses per-runner HTTPS long-poll — see [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md).

## Settings layering

`pi_dash/settings/`:

| File                                                  | Used for                                                      |
| ----------------------------------------------------- | ------------------------------------------------------------- |
| `common.py`                                           | Shared base — installed apps, middleware, DRF/Channels config |
| `local.py`                                            | Local dev overrides                                           |
| `production.py`                                       | Production defaults — `manage.py` defaults to this            |
| `test.py`                                             | Test-only overrides — referenced by `pytest.ini`              |
| `mongo.py` / `redis.py` / `storage.py` / `openapi.py` | Subsystem-specific bundles imported by the env files          |

`manage.py` sets `DJANGO_SETTINGS_MODULE=apple_pi_dash.settings.production` as the default. For local work, export `apple_pi_dash.settings.local`.

## Runner backend module (`pi_dash/runner/`)

This is the Django half of the runner integration — distinct from the Rust crate at the repo root. Highlights:

```
models.py              ← Runner, Run, Pod, Token models
views/
  register.py          ← runner enrollment (legacy token flow)
  runners.py           ← runner CRUD
  pods.py              ← pod (sub-resource) management
  runs.py / run_endpoints.py  ← run lifecycle endpoints
  approvals.py         ← approval router
  chat.py              ← chat / message endpoints
  sessions.py          ← session create / welcome frame
  enrollment.py        ← device-code endpoints
services/
  session_service.py   ← welcome-frame generation, version advisory
  run_lifecycle.py     ← state transitions
  tokens.py            ← refresh ↔ access token pair
  matcher.py           ← assign runs to runners
  outbox.py            ← outbound message queue
  pubsub.py            ← Redis pub/sub bridge for live updates
consumers.py           ← Channels WS consumer (legacy v3 + still-live editor channels)
routing.py             ← WS URL routes
```

The session service is responsible for the `welcome` frame the runner receives on connect — including `latest_runner_version` / `min_runner_version` advisories (see [15 — Releasing](./15-releasing.md)).

## Background work (Celery)

- Entry: `pi_dash/celery.py`.
- Broker: **RabbitMQ** (per `.env.example`).
- Tasks live in `pi_dash/bgtasks/` plus task modules under feature apps.

## Database

- **Postgres 15+** is the primary store.
- `pytest.ini` configures `--reuse-db --nomigrations` — tests reuse the DB across runs and **skip migrations**. This makes the test suite fast but means schema-altering work has to be tested explicitly with `--create-db`.

## Linting & formatting

Ruff handles Python format + lint (line-length **120**, `known-first-party = ["apple_pi_dash"]`). Ruff is **not** wired into the root `pnpm fix` — run it explicitly:

```bash
cd apps/api
ruff format .
ruff check . --fix
```

## Where to read next

- [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md) — the wire contract `pi_dash/runner/` implements
- [09 — Agent orchestration](./09-agent-orchestration.md) — how `orchestration/` + `prompting/` produce runs
- [11 — Authentication](./11-authentication.md) — the `authentication/` module + runner token model
