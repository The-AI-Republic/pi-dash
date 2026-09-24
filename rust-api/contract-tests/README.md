# Contract tests: Django behavior oracle for the Rust port

Each `<domain>/` suite pins the live Django backend's behavior —
same URL paths, same bytes — so the Rust server can be checked against
the same suite through the proxy. Never import Django or use its test
client here; the suite speaks HTTP (httpx) to a live server and seeds
data straight into Postgres.

Shared helpers live in `_harness/` (extend it, never fork per-domain
copies). Python `pytest` + `httpx` suites, one directory per domain
(`rust-api/contract-tests/<domain>/`), with shared helpers in `_harness/`
(created on first use; extend it, never fork it).
copies). Layout:

- `_harness/`   shared helpers (created once, extended per domain, never forked)
- `<domain>/`   one pytest package per ported domain (e.g. ``space/``)

The same suite runs against Django today and against the Rust server
through the proxy tomorrow. Nothing here may import Django or touch its
test client: HTTP goes through httpx, seeding goes straight into
Postgres via ``psycopg``, and authenticated sessions come from the real
sign-in endpoint (black box).

## Layout

- `_harness/` — shared helpers. Extend, never fork: `env.py` (env
  plumbing), `seed.py` (row factories + cleanup), `djangocrypto.py`
  (stdlib-only session/machine-token forging), `client.py`, `db.py`,
  `broker.py` (Celery protocol-v2 publish), `sinks.py` (recording
  HTTP stub), plus `config.py` (BASE_URL/DATABASE_URL env config),
  `auth.py` (HTTP session login), `factory.py`
  (user/workspace/notification factories) first added for
  PIDASHCONV-92.
- `web_edge/` — PIDASHCONV-14: web edge (`/`, `/robots.txt`).
- `v1_cli_auth/` — PIDASHCONV-80: api-v1 CLI auth + runner v1.
- `app_views_search/` — PIDASHCONV-87: app-tier views (project/global
  CRUD, view issues, favorites) + search (global, issue, entity) with
  FTS EXPLAIN parity on `issues_fts_idx`. Same env contract; any free
  port works (this suite was validated on 8124).
- `app_modules/` — PIDASHCONV-86 (D-28): app-tier modules CRUD,
  module-issue links, module links, favorites, user properties,
  archive/unarchive + archived list/detail. Same env contract; any
  free port works (this suite was validated on 8126; the dev server
  needs a Celery broker — `AMQP_URL=redis://127.0.0.1:6379/<db>` —
  because module writes publish activity tasks).
- `integrations/` — PIDASHCONV-19 (D-05 task oracle): black-box
  Celery-task oracle for the integrations library + git sync domain
  (file layout below).
- `app_notifications/` — PIDASHCONV-92 (D-34): app-tier notification
  list/detail/partial-update/destroy, read/unread, archive/unarchive,
  unread counts, mark-all-read, preferences get/patch. Same env
  contract, except no `CONTRACT_SECRET_KEY`/`CONTRACT_WEB_URL`: app
  endpoints use Django session auth (`session-id` cookie), so the
  harness logs in through the public flow — `GET /auth/get-csrf-token/`
  then `POST /auth/sign-in/` with a seeded `pbkdf2_sha256` password
  hash — and sends the session cookie as an explicit `Cookie` header.
  Seeded users get `user_timezone = "UTC"`; every run seeds under a
  unique tag (`ctn<hex8>`) and deletes its rows at teardown.
- `assistant/` — PIDASHCONV-20: assistant + SSE (threads, messages, SSE
  events, cancel, generate-title, BYOK/STT config + test, transcribe,
  agent profile/token, MCP servers). Same env contract; this suite was
  validated with a fernet BYOK key, a Redis-backed Celery broker, and
  the SSRF guard on (see "Backend under test (assistant domain)"
  below).
- `app_pages/` — PIDASHCONV-88 (D-30): app-tier pages (summary,
  list/create, retrieve/partial-update/destroy, favorite create/destroy,
  archive/unarchive, lock/unlock, access, description retrieve/patch,
  versions list/detail, duplicate). Same env contract, except no
  `CONTRACT_SECRET_KEY`/`CONTRACT_WEB_URL`: app endpoints use Django
  session auth (`session-id` cookie) via the public sign-in flow.
  Server knobs: `WEB_URL`/`APP_BASE_URL` must point at the server under
  test (sign-in redirects there); `CELERY_TASK_ALWAYS_EAGER=True` when
  no broker/worker is available (page writes enqueue `page_transaction`,
  which is DB-only, so eager is faithful); the harness rides out the
  stock `anon` 30/min throttle with a bounded Retry-After-honoring
  retry, so a full run goes green against stock settings.

## Run: web_edge

```sh
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt

# Terminal 1: live Django server (from the repo root checkout)
cd apps/api
DATABASE_URL="postgresql://postgres@/pidash_contract?host=/tmp&port=5434" \
  WEB_URL=http://localhost APP_BASE_URL=http://localhost \
  EMAIL_HOST=localhost API_KEY_RATE_LIMIT="100000/minute" \
  SECRET_KEY=<a fixed test-only secret> \
  AMQP_URL=memory:// \
  DJANGO_SETTINGS_MODULE=pi_dash.settings.test \
  python manage.py runserver 127.0.0.1:8000

# Terminal 2: the suite (from this directory)
BASE_URL=http://127.0.0.1:8000 DATABASE_URL="postgresql://..." pytest web_edge
```

`BASE_URL` is required. `DATABASE_URL` is required only by domains that
seed data straight into Postgres (web_edge needs no DB).

## Run against Django (DB-backed domains)

```sh
cd rust-api/contract-tests
python3.12 -m venv .venv && .venv/bin/pip install -r requirements.txt

# Fresh database, migrated by Django itself:
psql postgres -c "CREATE DATABASE pidash_contract_80;"
cd ../../apps/api
SECRET_KEY=<fixed> DATABASE_URL="postgresql://<user>@/pidash_contract_80?host=/tmp" \
  WEB_URL=http://127.0.0.1:8123 APP_BASE_URL=http://127.0.0.1:8123 \
  REDIS_URL=redis://127.0.0.1:6379/1 \
  python manage.py migrate --no-input
SECRET_KEY=<same> DATABASE_URL=... WEB_URL=... APP_BASE_URL=... REDIS_URL=... \
  python manage.py runserver 127.0.0.1:8123

# The suite:
cd ../../rust-api/contract-tests
BASE_URL=http://127.0.0.1:8123 \
DATABASE_URL="postgresql://<user>@/pidash_contract_80?host=/tmp" \
CONTRACT_SECRET_KEY=<same SECRET_KEY> \
CONTRACT_WEB_URL=http://127.0.0.1:8123 \
.venv/bin/pytest v1_cli_auth -q
```

Env contract:

| var | meaning |
|---|---|
| `BASE_URL` | server under test (default `http://127.0.0.1:8000`) |
| `DATABASE_URL` | psycopg conninfo for seeding (required) |
| `CONTRACT_SECRET_KEY` | must equal the server's `SECRET_KEY`; only used to forge session cookies / machine-token hashes when seeding (required) |
| `CONTRACT_WEB_URL` | must equal the server's `WEB_URL`; expected `verification_uri` base (required) |

## Throttle budget

The suite stays under Django's default brakes so a clean checkout passes
in one go: ~15 `device/start` calls (20/minute/IP), ~11 anonymous token
polls (30/minute/IP), a handful of requests per API key (60/minute/key).
Do not add start/token-poll calls casually; share fixtures instead.
Re-running within the same minute can 429 — wait for the window to slide.

## Run: integrations (D-05 task oracle, PIDASHCONV-19)

Black-box Celery-task oracle for the integrations library + git sync domain.
Nothing here imports Django. Tests publish jobs in Celery wire format to the
broker named by `CELERY_BROKER_URL`, let the live Django worker execute them,
and diff Postgres via `DATABASE_URL`.

```sh
cd rust-api/contract-tests
BASE_URL=http://api:8000 \
DATABASE_URL=postgresql://pidash:<pw>@db:5432/pidash \
CELERY_BROKER_URL=amqp://pidash:<pw>@mq:5672/pidash \
  pytest integrations -q
```

In this repo's docker dev stack the suite runs from a container on the
stack network so `db`/`mq`/`api` resolve; from the host use the published
ports. The checkout is mounted read-only at `/repo` for the static pins in
`test_beat.py`:

```sh
docker run --rm --network <stack>_default \
  -v $PWD/rust-api/contract-tests:/suite:ro -v $PWD:/repo:ro \
  -e DATABASE_URL=postgresql://pidash:<pw>@db:5432/pidash \
  -e CELERY_BROKER_URL=amqp://pidash:<pw>@mq:5672/pidash \
  -e BASE_URL=http://api:8000 \
  python:3.12-slim bash -c "pip install -q 'pika>=1.3' 'psycopg[binary]>=3.1' 'pytest>=8' \
    && cd /suite && python -m pytest integrations -q -p no:cacheprovider"
```

## Layout

- `_harness/` — shared helpers (first use; extend, never fork): `broker.py`
  (protocol-v2 publish, passive depth, drain-wait), `db.py` (raw SQL,
  `wait_for` polling), `sinks.py` (recording HTTP stub).
- `integrations/conftest.py` — env, read-only anchor rows, per-test `Scope`
  teardown that deletes every seeded row.
- `integrations/seed.py` — raw-SQL seed builders.
- `integrations/test_fanout.py` — beat fan-out tasks (payload/ack parity,
  disabled/empty no-ops, 4xx error-record + degrade).
- `integrations/test_single.py` — unknown-id no-ops (both providers),
  completion-comment idempotency + auth-failure error record, setup-ordering
  pin (unsupported provider fails silently), redelivery guards.
- `integrations/test_beat.py` — static pins read from the `/repo` checkout:
  beat entry identity, retry schedule, signal-hook wiring.

## Known oracle limits (pinned in stage 1)

- Provider HTTP has no black-box stub seam: the GitHub adapter hardcodes
  `api.github.com`; GitLab requires https + an allowlisted host. Happy-path
  sync diffs (mirror rows created from listings) cannot run hermetically, so
  the suite pins the deterministic surface: fan-out set, no-op cases, 4xx
  error recording (no retry), completion-comment idempotency, beat entry
  identity, redelivery guards.
- Live retry needs a real transient (e.g. provider 5xx → generic-except →
  `self.retry` with countdown 60 * 2^retries, max_retries=3), which has no
  hermetic trigger from outside; the schedule is pinned statically in
  `test_beat.py`. Probing the unknown-provider path instead revealed a real
  ordering behavior the suite now pins: adapter lookup runs before the
  guarded `try`, so an unsupported provider fails with no record and no
  retry — in-try faults record + retry. The Rust port must keep this order.
- The completion-hook signal (`github_signals`) fires only via an
  authenticated HTTP state transition (SessionAuthentication), so firing is
  pinned on the source (dispatch_uids, completed-group transition check,
  already-commented skip, one `.delay()` per mirror type) while the delayed
  tasks themselves are proven executable via the broker in `test_single.py`.
- The domain beat entry (`github-issue-sync-every-4h`, 4h cadence) is not
  wall-clock observable in a test run: parity is pinned on entry identity
  (name → task → crontab) plus the target task being registered/executable.
- The deliberate-permission-removal check from the HTTP coverage floor has no
  library-domain equivalent (no permission classes); recorded as n/a.

`SECRET_KEY` must be pinned to a fixed test-only value before the server
starts: `_harness.auth` forges DB-backed session rows with HMACs keyed by
it (stdlib only, no Django import), so rotating server keys would
invalidate seeded sessions. `AMQP_URL=memory://` lets the export
endpoints enqueue their Celery tasks without a broker; no worker
consumes them.
Domains that mint auth sessions also require `SECRET_KEY` (`CONTRACT_SECRET_KEY`
on suites using the newer `_harness.env` contract — same value, same meaning):

```sh
cd rust-api/contract-tests
pip install -r requirements.txt
BASE_URL=http://127.0.0.1:8000 \
DATABASE_URL=postgresql://postgres:postgres@localhost:5432/pidash \
SECRET_KEY=<the backend's Django SECRET_KEY> \
  pytest app_scheduler

DB-seeding domains need the backend's Django `SECRET_KEY` as well:

```sh
cd rust-api/contract-tests
BASE_URL=http://127.0.0.1:8000 \
DATABASE_URL=postgresql://postgres:postgres@localhost:5432/pidash \
SECRET_KEY=<the backend's Django SECRET_KEY> \
  pytest <domain>
```

The suite runs against a **live** backend and seeds rows straight into
Postgres over `DATABASE_URL`. It never imports Django and never uses its
test client. The same suite runs against the Rust server through the proxy.

`SECRET_KEY` must match the backend under test: suites mint `session-id`
cookies with the backend's session-signing format instead of calling login
endpoints, so auth behaves identically on both backends.

Worker-path suites (`loop`) need two more variables plus a live worker:

- `RABBITMQ_URL` — AMQP URL of the backend's broker, e.g.
  `amqp://pidash:secret@127.0.0.1:5672/pidash`. Tests publish Celery tasks
  in wire format with a throwaway client and observe the effects over HTTP
  and `DATABASE_URL`.
- `RABBITMQ_MGMT_URL` — base URL of the broker's management API, e.g.
  `http://127.0.0.1:15672`. Tests assert queued-task wire format
  (task name, args) with peek-and-requeue reads that never consume.
- The worker must consume a **dedicated queue** (the suite publishes to it;
  default `ct17`, override with `WORKER_QUEUE`) and must **not** consume the
  default `celery` queue, so downstream tasks the suite only observes
  (`assistant.run_turn`) pile up instead of executing.
- The backend runs with `LOOP_RECONCILE_EVERY_MINUTES=1` so the scanner's
  reconcile branch runs on every tick instead of once per 15 minutes; the
  throttle timing itself is Django-unit covered.

```sh
BASE_URL=... DATABASE_URL=... SECRET_KEY=... \
RABBITMQ_URL=amqp://pidash:secret@127.0.0.1:5672/pidash \
RABBITMQ_MGMT_URL=http://127.0.0.1:15672 \
WORKER_QUEUE=ct17 \
  pytest loop
```

## Conventions

- One test module per URL group plus `test_permissions.py` (denied cases)
  and `test_tenant_isolation.py`.
- Every endpoint gets a response-shape assertion (exact key sets).
- One denied-permission case per restricted endpoint; the suite must fail
  when a permission class is deliberately removed.
- Tests seed uniquely-named rows (uuid tags) and never truncate tables, so
  reruns against the same database are safe.
- Raw-SQL seeding bypasses model signals (no builtin-scheduler seeding, no
  default-pod creation): each test sees exactly what it inserted.

The server under test must be the full stack: Postgres (seeded directly),
Redis + a Celery worker (write endpoints enqueue activity tasks), and S3
credentials sufficient to mint presigned URLs offline (asset GET/POST pin
the redirect/upload-data shapes, never real objects).

Task-oracle suites (e.g. ``dispatch/``) additionally need:

```sh
CELERY_BROKER_URL=redis://localhost:6379/0  # or AMQP_URL; redis scheme only
```

pointing at the same broker the worker consumes, and the server must run
with ``CLOUD_AGENT_ENABLED=true`` (dispatch execution branches and the
queue scanner are kill-switched otherwise).

## Backend under test (assistant domain)

The assistant suite needs a live backend plus the Postgres it reads/writes.
Against Django (local run):

```sh
# Postgres + Redis running locally; then from apps/api:
DJANGO_SETTINGS_MODULE=pi_dash.settings.local \
DATABASE_URL=<same DATABASE_URL as above> \
REDIS_URL=redis://localhost:6379/5 \
SECRET_KEY=<fixed contract secret, see below> \
ASSISTANT_CRYPTO_BACKEND=fernet \
ASSISTANT_ENCRYPTION_KEY=<fernet key> \
WEB_URL=<same origin as BASE_URL> \
AMQP_URL=redis://localhost:6379/6 \
ASSISTANT_BLOCK_PRIVATE_URLS=True \
python -m uvicorn pi_dash.asgi:application --host 127.0.0.1 --port 8891
```

and for the suite itself:

```sh
export CONTRACT_DJANGO_SECRET_KEY=<same fixed contract secret>
```

Notes:

- Use private Redis DB numbers: the default DB is shared with other local
  services, which would merge throttle counters and pubsub traffic.
- `SECRET_KEY` must be fixed (and mirrored in `CONTRACT_DJANGO_SECRET_KEY`)
  because the harness mints session cookies with the backend's exact session
  encoding instead of driving the sign-in form — hundreds of per-test form
  logins would trip the 30/minute anonymous throttle. Sign-in itself is the
  authentication domain's contract, not this suite's.
- `AMQP_URL=redis://…` lets the message POST dispatch its Celery task without
  a worker; the turn stays queued, which is exactly what the suite asserts.
- `ASSISTANT_BLOCK_PRIVATE_URLS=True` is cloud parity; the SSRF cases pin
  the guard, and all other saved URLs are literal public IPs so no DNS is
  needed.
- Every test seeds uniquely-suffixed rows and never tears down: the suite is
  re-runnable and parallel-safe (`pytest -n auto` works).

## Conventions for new domains

- `BASE_URL` / `DATABASE_URL` from the environment; never hardcode hosts.
- Seed with SQL via `_harness` (`build_world`, `login_client`); never import
  Django or use its test client.
- Every endpoint gets a response-shape assertion, plus one denied-permission
  case and one tenant-isolation case per domain; the suite must fail if a
  permission class is removed.
- Outbound-provider behaviour that needs a live third party stays out;
  cover those endpoints through their deterministic gates and error shapes.

## Domain: v1_cycles_modules (D-20 oracle, PIDASHCONV-78)

- `v1_cycles_modules/` — D-20 oracle: api-v1 cycles (8 routes) + modules
  (7 routes). Fixtures live in the domain `conftest.py` (following the
  app_scheduler/ precedent); the root `conftest.py` stays a plain import
  shim for every suite.
- `_harness/api.py` — httpx clients + api-v1 URL builders (added here;
  extend, never fork).
- `_harness/db.py` — also carries the D-20 fixed-UUID seed set:
  `reset()` / fixed-UUID seeds (same module as the baseline helpers, never
  a per-domain fork).
- `_harness/contract_eager_settings.py` — local-boot Django settings shim
  (NOT imported by the suite): test settings plus eager Celery and
  in-memory cache so `runserver` works with no RabbitMQ/Redis. CI uses
  real services instead; HTTP behaviour is identical.

## Run against local Django (eager shim, D-20)

```sh
# 1. Postgres (any instance; must be migrated — see 3.)
export DATABASE_URL=postgres://postgres:postgres@localhost:5433/pidash_contract

# 2. Python env with the API deps
python3.12 -m venv /tmp/pidash-venv
/tmp/pidash-venv/bin/pip install -r ../../apps/api/requirements/test.txt
/tmp/pidash-venv/bin/pip install -r requirements.txt

# 3. Migrate
cd ../../apps/api
DATABASE_URL=$DATABASE_URL DJANGO_SETTINGS_MODULE=pi_dash.settings.test \
  /tmp/pidash-venv/bin/python manage.py migrate

# 4. Serve (from the repo root; the shim lives in _harness/)
cd ../..
PYTHONPATH=apps/api:rust-api/contract-tests/_harness DATABASE_URL=$DATABASE_URL \
  DJANGO_SETTINGS_MODULE=contract_eager_settings \
  /tmp/pidash-venv/bin/python apps/api/manage.py runserver 127.0.0.1:8471 --noreload

# 5. Run the suite (from THIS directory — paths assume that cwd)
cd rust-api/contract-tests
BASE_URL=http://127.0.0.1:8471 DATABASE_URL=$DATABASE_URL \
  /tmp/pidash-venv/bin/python -m pytest v1_cycles_modules
```

## Coverage contract (every domain suite)

1. A shape assertion for every routed drf-spectacular endpoint.
2. One denied-permission case (expects 403).
3. One tenant-isolation case (a valid token from another workspace, 403).
4. The suite must fail when a permission class is deliberately removed —
   demonstrated per domain by a one-line local patch (reverted, never
   committed); see the domain PR for the transcript.

## Ported upstream bugs (D-20, reproduced byte-for-byte)

- `GET .../cycles/?cycle_view=current` returns a bare JSON list, every
  other view returns the paginated envelope.
- `POST .../cycles/<draft>/archive/` (null `end_date`) → 500
  `{"error": "Something went wrong please try again later"}`.
- `PATCH` on a completed cycle with `{"sort_order": N}` → 200 but the
  value is silently dropped (serializer has no `sort_order` field).
- Deletes are soft (`deleted_at` set, row retained).
