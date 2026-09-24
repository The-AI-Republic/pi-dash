# Contract tests (stage 1 oracles)

Black-box HTTP suites, one directory per domain. The same suite runs
against the live Django backend today and against the Rust backend
through the proxy tomorrow. No Django imports, no Django test client:
`httpx` for HTTP, raw SQL via `psycopg` for seeding.

## Layout

- `_harness/` — shared helpers. Extend, never fork: `env.py` (env
  plumbing), `seed.py` (row factories + cleanup), `djangocrypto.py`
  (stdlib-only session/machine-token forging).
- `v1_cli_auth/` — PIDASHCONV-80: api-v1 CLI auth + runner v1.
- `app_views_search/` — PIDASHCONV-87: app-tier views (project/global
  CRUD, view issues, favorites) + search (global, issue, entity) with
  FTS EXPLAIN parity on `issues_fts_idx`. Same env contract; any free
  port works (this suite was validated on 8124).

## Run against Django

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
