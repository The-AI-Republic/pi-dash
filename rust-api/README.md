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

## Cutover edge (F-02)

`serve` sits in front of Django: every request is reverse-proxied to
`PIDASH_DJANGO_UPSTREAM` (default `http://127.0.0.1:8000`) unless its URL
prefix is flipped to Rust. The routing table in `crates/api/src/edge.rs`
encodes the inventory §0 prefix map; each row has a flag, all default off:

| Env var | Prefix | Django module |
|---|---|---|
| `PIDASH_RUST_WEB` | site root (`/`, `/robots.txt`) | `web.urls` |
| `PIDASH_RUST_APP` / `ASSISTANT` / `LOOP` / `PROMPTING` | `api/` | `app`/`assistant`/`loop`/`prompting.urls` |
| `PIDASH_RUST_SPACE` | `api/public/` | `space.urls` |
| `PIDASH_RUST_LICENSE` | `api/instances/` | `license.urls` |
| `PIDASH_RUST_RUNNER_WEB` | `api/runners/` | `runner.web_urls` |
| `PIDASH_RUST_API_V1` | `api/v1/` | `api.urls` |
| `PIDASH_RUST_RUNNER` | `api/v1/runner/` | `runner.urls` |
| `PIDASH_RUST_AUTH` | `auth/` | `authentication.urls` |

Values `1/true/yes/on` flip a prefix; anything else (or absent) proxies.
Longest prefix wins (`api/v1/runner/` beats `api/v1/`). Today only `WEB`
has Rust handlers (`GET /` → `{"status": "OK"}`, `GET /robots.txt`,
byte-identical to Django); every other method on those paths still proxies
so Django's CSRF-failure page is preserved exactly. Upstream outages answer
`502 {"error": {"code": "bad_gateway", ...}}`.

Rollback drill (recorded in the F-02 PR): with Django on `:8000`,

```sh
export PIDASH_DJANGO_UPSTREAM=http://127.0.0.1:8000
cargo run -p pidash-api-bin -- serve --bind 127.0.0.1:8080 &  # all flags off
curl localhost:8080/                # Django's bytes, via proxy
export PIDASH_RUST_WEB=1            # + restart: Rust serves / and /robots.txt
curl localhost:8080/                # {"status": "OK"}, identical bytes
unset PIDASH_RUST_WEB               # + restart: traffic returns to Django
curl localhost:8080/                # Django's bytes again
```

## Reference

- PIDASHCONV-1: the rulebook (rules, stages, issue types, where things live).
- Porting guide wiki page: crate layout, Django idiom → Rust pattern table,
  semantic traps. Foundation issues and pilots update it.
