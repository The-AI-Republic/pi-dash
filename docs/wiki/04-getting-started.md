# 04 — Getting Started Locally

This is the from-zero contributor onboarding. For the production install paths see the top-level `README.md`.

## Requirements

- **Docker Engine** running locally
- **Node.js ≥ 22.18 LTS**
- **Rust toolchain** (stable; `rust-toolchain.toml` pins the version — the Rust API and the runner both build with cargo)
- **Postgres 15+** (provided by docker-compose-local)
- **Valkey 7+** (or Redis 7+, drop-in compatible — also provided)
- **RAM ≥ 12 GB** — 8 GB will OOM during Docker build / pnpm install

The active system needs no Python and no RabbitMQ: background jobs run on the Postgres queue drained by the Rust worker.

## First-time bootstrap

```bash
git clone https://github.com/The-AI-Republic/pi-dash.git
cd pi-dash
chmod +x setup.sh
./setup.sh
```

`setup.sh` does three things:

1. **Copies every `.env.example` to `.env`**. The defaults work out of the box for loopback dev (localhost URLs, `pi-dash` Postgres creds, local MinIO endpoint). Only edit `.env` files if you're binding to a non-default host or wiring in external services.
2. **Generates a unique `SECRET_KEY`** for session validation.
3. **Runs `pnpm install`** for the TS workspace.

## Start the infra

```bash
docker compose -f docker-compose-local.yml up pi-dash-db pi-dash-redis pi-dash-minio
```

This brings up just the dependencies — Postgres, Valkey and MinIO. The app processes themselves run on your host so you keep hot reload.

## Apply the schema (one-off)

```bash
docker compose -f docker-compose-local.yml up migrator
```

The migrator applies the migration history to the shared database and then exits — it is the only Python container, and it never stays running. (Temporary until NEWPIDASH-380 lands the native Rust migrator; services never run migrations themselves.)

## Start the apps (one terminal each)

```bash
cd rust-api
export DATABASE_URL="postgresql://pi_dash:pi_dash@127.0.0.1:5432/pi_dash"
export REDIS_URL="redis://127.0.0.1:6379"
cargo run -p pidash-api-bin -- serve --bind 127.0.0.1:8000
```

```bash
cd rust-api
export DATABASE_URL="postgresql://pi_dash:pi_dash@127.0.0.1:5432/pi_dash"
export REDIS_URL="redis://127.0.0.1:6379"
cargo run -p pidash-api-bin -- worker --concurrency 4
```

```bash
pnpm --filter live dev
pnpm --filter web_new dev
```

`serve` refuses to boot when the schema is not current — re-run the migrator step above if it exits. (`pnpm dev` without a filter starts every workspace app, including the frozen legacy frontends — prefer the filtered commands above. See the root `README.md` for the self-hosted production paths.)

### Ports

| App       | Port   | Purpose                                    |
| --------- | ------ | ------------------------------------------ |
| `web_new` | **3010** | Current product UI (incl. `/god-mode/`)  |
| `live`    | **3100** (per `.env`) | Hocuspocus realtime editor server |
| `api`     | **8000** | Rust API (`pidash-api serve`)            |

## First-run flow

1. Open **`http://localhost:3010`** and register yourself as the **instance admin**. You only do this once per instance.
2. Log in with the same credentials. This is the actual product.
3. Create a workspace → create a project → invite yourself.
4. (Optional, for runner work) install the `pidash` CLI and point it at `http://localhost:8000` — see [11 — Authentication](./11-authentication.md) for device-code login.

## Common gotchas

- **Rust API needs its env on every terminal.** `serve` and `worker` read `DATABASE_URL` (TCP URL, not a socket dir) and `REDIS_URL` from the environment — export them in each shell, or the server 500s every DB-backed route / degrades pub-sub.
- **`serve` fails fast on a stale schema.** It checks at boot and exits nonzero when migrations are pending — re-run the migrator, don't restart the server and hope.
- **Turbo concurrency = 18.** If a full `pnpm dev` is choking your machine, lower it: `pnpm turbo run dev --concurrency=8` — or stick to the filtered dev commands above.
- **`web_new` lints with `--deny-warnings`.** `pnpm --filter web_new lint` (and `check:boundaries`, `check:headers`, `check:deps`, `check:similarity`) must pass — fix the warning, don't work around the check.
- **Husky + lint-staged run `oxfmt` and `oxlint --fix --deny-warnings` on commit.** A hook failure means the commit didn't happen — fix the underlying issue and re-commit, do **not** `--no-verify`.
- **Postgres data persists** in the named Docker volume from `docker-compose-local.yml`. `docker compose down -v` will wipe it; that's how you get a clean slate.

## Where to go next

- [13 — Development workflow](./13-development-workflow.md) — pnpm/turbo/lint/format day-to-day commands
- [14 — Testing](./14-testing.md) — running pytest / vitest / cargo test
- [05](./05-frontend-architecture.md), [06](./06-backend-architecture.md), [07](./07-runner-architecture.md) — the per-stack deep dives
