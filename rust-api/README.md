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

## Config and settings (F-03)

`crates/db/src/config/` ports `pi_dash.config`:

- `registry` — the per-key env-vs-DB catalog (`ENV_KEYS_OVERRIDE_VAR`
  reclassifies keys to env, the cloud SSM seam).
- `accessor` — `get_config` / `get_many` / `get_bool` / `get_int` over a
  `ConfigStore` (Postgres: `PgConfigStore` over `instance_configurations`).
  Env-tier reads never touch the store; boot reads reject DB-tier keys.
- `encryption` — Fernet secrets, byte-compatible with Python's
  `encryption.py` (same PBKDF2 key schedule; decrypt failure yields `""`).
- `legacy` — the `get_configuration_value` batch shim (not the
  email-domain `get_email_configuration`, which travels with that domain).
- `settings` — boot-time `Settings` (`common.py`), `Profile`
  (`local`/`production`/`test` deltas), and `SettingsOverlay` for the
  private crate. Django-only furniture (`INSTALLED_APPS`, `MIDDLEWARE`,
  `CACHES`, log dirs) is out of scope.

## Composing the app builder (private overlay)

The builder lives in the library so a private crate's own `main.rs` can use
it — the binary is only a thin wrapper:

```rust
let settings = pidash_db::config::Settings::from_env()?;
let state = pidash_api::AppState::with_settings(env!("CARGO_PKG_VERSION"), settings);
let app = pidash_api::build_app(state, Some(private_routes()));
```

With an overlay: implement `SettingsOverlay` (`env_keys` to force keys to
env, `apply` to mutate the resolved `Settings`), pre-install the registry
with `try_init_global` when reclassification must be process-wide, and pass
`Settings::from_map_with(&vars, Profile::Production, &overlay)`.

## Extension seams (F-10)

The six `ee/` stubs are traits with CE defaults; the private crate
replaces the implementation, never the callers:

| Trait | Module | CE default | Python source |
|---|---|---|---|
| `AssistantModelSeam` | `pidash-services::extensions` | `ByokAssistantSeam` | `ee/assistant/model_provider.py` |
| `SttSeam` | `pidash-services::extensions` | `ByoSttSeam` | `ee/assistant/stt_provider.py` |
| `CloudAgentModelSeam` | `pidash-services::extensions` | `CreatorModelSeam` | `ee/cloud_agent/model_provider.py` |
| `CloudAgentToolsetsSeam` | `pidash-services::extensions` | `NoExtraToolsets` | `ee/cloud_agent/toolsets.py` |
| `DesktopGate` | `pidash-auth::permissions::desktop` | `SessionDesktopGate` | `ee/authentication/desktop.py` |
| `UserSettingsSchema` | `pidash-services::user_settings` | `CeUserSettings` | `ee/settings/user_settings.py` |

Named route-group replacement (`crates/api/src/overlay.rs`, one group per
§0 prefix): `Overlay::new().replace(RouteGroup::Auth, cloud_auth())` swaps
a whole group (the cloud auth replacement), `.drop(RouteGroup::Auth)`
removes every OSS mount in the group (`_strip_oss_auth`), and
`.add_routes(router)` shadows individual OSS paths — additive routes sit in
front of the OSS groups via fallback dispatch, the cloud pattern of placing
paths ahead of the OSS include. Model/toolset *construction* extends the
same traits when the assistant/cloud-agent runtimes are ported (D-06/D-11).

Private migrations live in the private crate at
`rust-api/private/migrations/` (`NNN_name.sql`, lexical order, applied
after the OSS baseline; tables prefixed `private_`; see
`crates/db/src/migrations.rs`). Private migrations never touch
Django-owned or OSS `rust_*` tables — the same rule Django's own
`pi_dash_cloud/*/migrations/` directories follow.

## Serializer + paginator kernel (F-07)

Every domain port serializes and paginates through these kernels:

- `crates/api/src/serializer.rs` — DRF-compatible JSON: `parse_list_param`
  (`fields`/`expand` query params), `render_datetime` / `render_datetime_in`
  (DRF `iso-8601`, `Z` for UTC, microseconds iff nonzero, request zone from
  `TimezoneMixin`), `render_decimal` (plain string, scale preserved),
  `effective_selection` + `apply_expansion` + `expand_value` /
  `expand_fallback_id` (`DynamicBaseSerializer`, `fields` kwarg discarded).
- `crates/api/src/paginator.rs` — `Cursor` (`value:offset:is_prev`),
  `parse_per_page`, `offset_window` / `grouped_window`,
  `PageResponse` (the 12-key envelope), `process_grouped_results` /
  `process_sub_grouped_results`, plus `offset_query` / `grouped_page_query`
  sea-query builders pinning the SQL shapes.
- `crates/db/src/filterset.rs` — the `IssueFilterSet` declaration
  (49 names incl. `__exact` aliases), `compile_leaf` / `build_combined`,
  `archived_condition`.
- `crates/db/src/issue_filters.rs` — the legacy `issue_filters` compiler
  (`issue_filters_get` / `issue_filters_post`, 25 keys in order).

Ported bugs live in the module docs and the PR body, not here.

## Reference

- PIDASHCONV-1: the rulebook (rules, stages, issue types, where things live).
- Porting guide wiki page: crate layout, Django idiom → Rust pattern table,
  semantic traps. Foundation issues and pilots update it.
