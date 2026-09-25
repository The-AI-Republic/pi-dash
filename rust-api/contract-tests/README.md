# Contract tests: Django behavior oracle for the Rust port

Each `<domain>/` suite pins the live Django backend's HTTP behavior —
same URL paths, same bytes — so the Rust server can be checked against
the same suite through the proxy. Never import Django or use its test
client here; the suite speaks HTTP (httpx) to a live server and seeds
data straight into Postgres.

Shared helpers live in `_harness/` (extend it, never fork per-domain
copies).

## Run

```sh
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt

# Terminal 1: live Django server (from the repo root checkout)
cd apps/api
DATABASE_URL="postgresql://postgres@/pidash_contract?host=/tmp&port=5434" \
  WEB_URL=http://localhost APP_BASE_URL=http://localhost \
  EMAIL_HOST=localhost API_KEY_RATE_LIMIT="100000/minute" \
  DJANGO_SETTINGS_MODULE=pi_dash.settings.test \
  python manage.py runserver 127.0.0.1:8000

# Terminal 2: the suite (from this directory)
BASE_URL=http://127.0.0.1:8000 DATABASE_URL="postgresql://..." pytest web_edge
```

`BASE_URL` is required. `DATABASE_URL` is required only by domains that
seed data straight into Postgres (web_edge needs no DB).
