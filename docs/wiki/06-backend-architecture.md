# 06 — Backend Architecture

The backend is the **Rust API** in `rust-api/`: axum routers in `crates/api/`, per-domain logic in `crates/services/`, background jobs in `crates/jobs/`, served by the `pidash-api` binary — `serve` for HTTP (port 8000), `worker` for the Postgres job queue. Full runbook: [`rust-api/README.md`](../../rust-api/README.md).

## Top-level layout

```
rust-api/
├── Cargo.toml                  ← own [workspace]; excluded from the root workspace
├── crates/types/               ← ids, enums, DTOs, error type; no I/O
├── crates/db/                  ← pools, sea-query builders, request context, tx wrapper, SeaORM entities
├── crates/auth/                ← session reader, token validation, permission kernel
├── crates/services/            ← per-domain logic; write side via pidash_services::domain
├── crates/api/                 ← axum routers, extractor + serializer + paginator kernels, edge proxy
├── crates/jobs/                ← Postgres queue, worker loop, scheduler, seed loader
├── crates/storage/             ← server-side S3 PUT/DELETE + avatar mapping
├── crates/sdk/                 ← pidash-sdk: the only surface the cloud overlay may use
├── bin/pidash-api/             ← the binary: serve + worker + ops modes
├── contract-tests/             ← pytest suites run against the Rust binary (Django is the oracle)
└── fixtures/                   ← recorded Python behaviour
```

Domain code lives at `crates/<layer>/src/<domain>/`, using the slug in each epic body. Dependencies point strictly downward: `types` → `db` → `auth` → `services` → `api`; `jobs` depends on `types` + `db`. Same URL paths, same JSON byte for byte, same schema, same SQL semantics as the old system. Every crate root has `#![forbid(unsafe_code)]`.

## Serve + worker

- **`serve`** connects the database at boot and refuses to start when it is missing, unreachable, or not current on migrations (fail fast — see [04 — Getting started](./04-getting-started.md)). `--bind` sets the listen address (`:8000` in containers, `:8080` bare default). `SECRET_KEY` from the environment validates sessions; unset, the server mints an ephemeral key (warned at boot).
- **`worker`** drains the `rust_job_queue` Postgres tables — scheduler, seeds, issue transitions, mail. No broker, no AMQP path, no RabbitMQ.
- **`ops`** subcommands (`ops wait_for_db`, `ops instance register-instance`, `ops create_bucket`, …) implement the container boot steps; `Dockerfile.api` + `bin/docker-entrypoint-*.sh` are the image layer.

## Edge proxy (Django fallback)

`crates/api/src/edge.rs` keeps a reverse proxy to the Django upstream (`PIDASH_DJANGO_UPSTREAM`, default `http://127.0.0.1:8000`). Each URL prefix has a `PIDASH_RUST_*` flip flag; a flagged prefix is served by its Rust handler, anything else proxies to Django. Production runs with all flags on — every request is served by Rust and Django stays only as the fallback. The shadow-read mode (`PIDASH_RUST_SHADOW`) compares Rust responses against Django byte for byte without serving them. See [Cutover edge](../../rust-api/README.md#cutover-edge-f-02) in the runbook.

## Settings

`crates/db/src/config/` carries the config system: the per-key env-vs-DB catalog, the `ConfigStore` over `instance_configurations`, Fernet-compatible secrets, and boot-time `Settings` with `local` / `production` / `test` profiles. The cloud overlay composes them through `SettingsOverlay` — see [Composing the app builder](../../rust-api/README.md#composing-the-app-builder-private-overlay).

## Contract tests

Backend behaviour is pinned by the pytest suites in `rust-api/contract-tests/<domain>/`. They run unmodified against live Django (the oracle: the old system's real behaviour) and against the Rust binary with every `PIDASH_RUST_*` flag on. A fix that changes pinned behaviour on purpose updates the case to expect the fixed behaviour on the new system and the old behaviour on the old one. Fixtures live in `rust-api/fixtures/<domain>/`.

## Schema

Django's migrations stay the single schema history so the frozen fallback can still run against the shared database. The Rust entities regenerate from the migrated schema (see the `entity` module docs in `crates/db/src/entity/`); the Rust code never runs its own DDL.

## Linting & formatting

```bash
cd rust-api
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test -p <affected-crate>
python3 scripts/write_guard.py --check
```

## Frozen Django fallback (`apps/api/`)

The Django 4 / DRF / Channels application in `apps/api/` is frozen and kept only as a fallback: no bug fixes, no new features, no behaviour change. What you need to orient in it:

```
apps/api/
├── manage.py
├── apple_pi_dash/   ← legacy Django project name (settings module path)
└── pi_dash/         ← app modules: app/ api/ authentication/ space/ license/
                        runner/ orchestration/ prompting/ scheduler/ analytics/
                        bgtasks/ (Celery — frozen) db/ logs/ middleware/
                        throttles/ seeds/ settings/ tests/ utils/ web/
```

> The Python package is named `pi_dash`, but `DJANGO_SETTINGS_MODULE` historically references `apple_pi_dash.settings.*`. They co-exist — don't rename them.

- URL tree (`pi_dash/urls.py`): `api/` (internal app API) · `api/public/` (spaces) · `api/instances/` (license) · `api/runners/` + `api/v1/runner/` (runner HTTP + WS) · `api/v1/` (public REST) · `auth/` · `/` (static catch-all).
- The runner WebSocket path is the legacy v3 transport; the current wire protocol (v4) uses per-runner HTTPS long-poll — see [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md).
- Settings: `pi_dash/settings/{common,local,production,test}.py`; `manage.py` defaults to `production`.
- Background work ran on Celery + RabbitMQ — frozen with the backend.

## Where to read next

- [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md) — the wire contract the Rust runner handlers implement
- [09 — Agent orchestration](./09-agent-orchestration.md) — how services + jobs produce runs
- [11 — Authentication](./11-authentication.md) — sessions, OIDC, and the runner token model
