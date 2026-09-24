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
```

The suite runs against a **live** backend and seeds rows straight into
Postgres over `DATABASE_URL`. It never imports Django and never uses its
test client. The same suite runs against the Rust server through the proxy.

`SECRET_KEY` must match the backend under test: suites mint `session-id`
cookies with the backend's session-signing format instead of calling login
endpoints, so auth behaves identically on both backends.

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
