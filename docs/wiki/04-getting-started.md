# 04 — Getting Started Locally

This is the from-zero contributor onboarding. For the production install paths see the top-level `README.md`.

## Requirements

- **Docker Engine** running locally
- **Node.js ≥ 22.18 LTS**
- **Python 3.12+** (Django backend runs in its own venv)
- **Postgres 15+** (provided by docker-compose-local)
- **Valkey 7+** (or Redis 7+, drop-in compatible — also provided)
- **RAM ≥ 12 GB** — 8 GB will OOM during Docker build / pnpm install
- **Rust toolchain** (only needed if you'll touch `runner/`; `rust-toolchain.toml` pins the version)

## First-time bootstrap

```bash
git clone https://github.com/The-AI-Republic/pi-dash.git
cd pi-dash
chmod +x setup.sh
./setup.sh
```

`setup.sh` does three things:

1. **Copies every `.env.example` to `.env`** — repo root plus `apps/{web,api,space,admin,live}`. The defaults work out of the box for loopback dev (localhost URLs, `pi-dash` Postgres creds, local MinIO endpoint). Only edit `.env` files if you're binding to a non-default host or wiring in external services.
2. **Generates a unique Django `SECRET_KEY`** and appends it to `apps/api/.env`.
3. **Runs `pnpm install`** for the TS workspace.

## Start the infra

```bash
docker compose -f docker-compose-local.yml up
```

This brings up just the dependencies — Postgres, Redis/Valkey, RabbitMQ, MinIO. The app processes themselves run on your host so you keep hot reload.

## Start the apps

```bash
pnpm dev
```

Turbo launches every dev server concurrently. Watch the output for any one that failed to start.

### Ports

| App     | Port                      | Purpose                           |
| ------- | ------------------------- | --------------------------------- |
| `web`   | **3000**                  | Main product UI                   |
| `admin` | **3001**                  | Instance admin ("god mode")       |
| `space` | **3002**                  | Published public spaces           |
| `live`  | per `.env`                | Hocuspocus realtime editor server |
| `api`   | (Django, port per `.env`) | REST + Channels backend           |

## First-run flow

1. Open **`http://localhost:3001/god-mode/`** and register yourself as the **instance admin**. You only do this once per instance.
2. Open **`http://localhost:3000`** and log in with the same credentials. This is the actual product.
3. Create a workspace → create a project → invite yourself.
4. (Optional, for runner work) install the `pidash` CLI and point it at `http://localhost:8000` (or whatever the API exposes locally) — see [11 — Authentication](./11-authentication.md) for device-code login.

## Common gotchas

- **Django defaults to `production` settings.** `manage.py` sets `DJANGO_SETTINGS_MODULE=pi_dash.settings.production`. For local work either export `DJANGO_SETTINGS_MODULE=pi_dash.settings.local` or use the helper scripts under `apps/api/`.
- **Turbo concurrency = 18.** If `pnpm dev` is choking your machine, lower it: `pnpm turbo run dev --concurrency=8`.
- **OxLint warning ceilings will block your commit.** Each app pins a `--max-warnings` ceiling in its `package.json`. Adding warnings past the ceiling fails `check:lint` and trips the pre-commit hook. Either fix the warning or — if you legitimately removed some — _lower_ the ceiling, don't raise it.
- **Husky + lint-staged run `oxfmt` and `oxlint --fix --deny-warnings` on commit.** A hook failure means the commit didn't happen — fix the underlying issue and re-commit, do **not** `--no-verify`.
- **Postgres data persists** in the named Docker volume from `docker-compose-local.yml`. `docker compose down -v` will wipe it; that's how you get a clean slate.

## Where to go next

- [13 — Development workflow](./13-development-workflow.md) — pnpm/turbo/lint/format day-to-day commands
- [14 — Testing](./14-testing.md) — running pytest / vitest / cargo test
- [05](./05-frontend-architecture.md), [06](./06-backend-architecture.md), [07](./07-runner-architecture.md) — the per-stack deep dives
