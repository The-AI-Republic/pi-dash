# Pi Dash Rust backend (`rust-api/`)

Rust port of the Django backend (`apps/api`), taking over traffic behind a
proxy one domain at a time. Python stays in place; nothing is deleted.

Quarantine: code lives only under `rust-api/`. The only files allowed outside
it are `.github/workflows/rust-api*.yml` and the one-line `exclude` in the
root `Cargo.toml`. Foundation crates (`crates/{types,db,auth,services,api,jobs}`)
are read-only for port agents; a needed change there is a new issue.

## Layout

```text
rust-api/
  Cargo.toml            # own [workspace]; excluded from the root workspace
  crates/types/         # ids, enums, DTOs, error type; no I/O
  crates/db/            # pools, sea-query builders, soft-delete views, request context, tx wrapper
  crates/auth/          # session reader, token validation, permission kernel
  crates/services/      # per-domain logic; depends on types, db, auth
  crates/api/           # axum routers, extractors, serializer + paginator kernel, middleware
  crates/jobs/          # Postgres queue, worker loop, Celery-format publisher
  bin/pidash-api/       # the binary: serve + worker modes, app builder with extension seams
  contract-tests/       # stage-1 pytest suites (+ _harness/)
  fixtures/<domain>/    # recorded Python behaviour
```

Domain code lives at `crates/<layer>/src/<domain>/`, using the slug in each
epic body. Dependencies point strictly downward:
`types` → `db` → `auth` → `services` → `api`; `jobs` depends on `types` + `db`.

Rules: same URL paths, same JSON byte for byte, same schema, same SQL
semantics as Django. Port existing bugs and list them in the PR. No stubs, no
skipped or weakened tests. Every crate root has `#![forbid(unsafe_code)]`.

## Build

```sh
cd rust-api
cargo build --workspace
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Run the contract suite

Contract suites run unmodified against live Django today and against Rust
through the proxy tomorrow. They need a live backend plus Postgres:

```sh
cd rust-api/contract-tests
python3 -m venv .venv && source .venv/bin/activate && pip install -e .
BASE_URL=http://localhost:8000 DATABASE_URL=postgresql:///pidash_contract pytest app_intake
```

See [contract-tests/README.md](contract-tests/README.md) for the Django
server (oracle) setup. Never import Django or use its test client.

## Run serve / worker locally

```sh
cd rust-api
cargo run -p pidash-api-bin -- serve --bind 127.0.0.1:8080
curl localhost:8080/healthz   # {"status":"ok","version":"0.1.0"}

cargo run -p pidash-api-bin -- worker --concurrency 4
```

`DATABASE_URL` must point at your own scratch database; print the target
before any migrate, seed or destructive SQL and stop if it is not yours.
Never touch the shared postgres database.

## Reference

- PIDASHCONV-1: the rulebook (rules, stages, issue types, where things live).
- Porting guide wiki page: crate layout, Django idiom → Rust pattern table,
  semantic traps. Foundation issues and pilots update it.
