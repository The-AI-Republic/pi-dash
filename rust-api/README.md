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
  crates/storage/       # shared server-side S3 PUT/DELETE (offline SigV4) + avatar mapping (PIDASHCONV-480)
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

## Local checks

GitHub CI is advisory -- including the `rust-api` and `rust-api-write-guard`
checks that run on pull requests into `rust-dev`. It does not gate merge, per
the rulebook; but a red PR check must be explained in the PR body before merge.
Run these locally before pushing:

```sh
cd rust-api
python3 scripts/write_guard.py --check
```

The write guard (ORM-09, design D7) fails when raw SQL or sea-query writes
to the ORM-covered tables appear outside `pidash_services::domain`. New
sites must move into `domain/`; the allowlist only shrinks.

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

`serve` connects the database at boot and refuses to start when it is
missing or unreachable (a pool-less server 500s every DB-backed route, so
fail-fast beats serving). Use a TCP URL
(`postgresql://user:pass@127.0.0.1:5432/db`); socket-dir URLs
(`?host=/tmp`) are rejected by the URL parser at connect time. `serve`
also honors `SECRET_KEY` from the environment for session validation: when
it is unset the server mints an ephemeral key (warned at boot) and every
previously forged session stops validating, so contract runs must export
the same `SECRET_KEY` the sessions were forged with.

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

## Shadow reads (PIDASHCONV-822)

Stage-8 validation: with `PIDASH_RUST_SHADOW=1` (default off), a `GET` or
`HEAD` on a prefix whose flag is OFF still returns Django's response to the
client byte-identically, and additionally computes the Rust handler's
response for the same request in the background and compares the two. The
client never waits on the shadow call. Code:
`crates/api/src/edge.rs` (proxy hook) and `crates/api/src/edge_shadow.rs`.

| Env var | Default | Meaning |
|---|---|---|
| `PIDASH_RUST_SHADOW` | off | `1/true/yes/on` enables shadow reads |
| `PIDASH_RUST_SHADOW_MAX_INFLIGHT` | `8` | concurrent shadow cap; over it, skip and count |
| `PIDASH_RUST_SHADOW_SAMPLE` | `1.0` | sampling rate, clamped to `0.0`–`1.0` |
| `PIDASH_RUST_SHADOW_METRICS_TOKEN` | (unset) | shared secret unlocking `/internal/shadow-metrics`; unset locks it for everyone |

Eligibility: safe reads only. Writes (`POST`/`PUT`/`PATCH`/`DELETE`)
never shadow and leave no trace in metrics, `X-Api-Key` requests never
shadow (both backends stamp `last_used` on every one, so a re-dispatch
would double-write), and the denylist in `edge_shadow.rs` (`DENYLIST`)
excludes `GET`s with side effects: the CSRF-token mint, OAuth
initiates/callbacks (all four providers, app + space), and the
presigned-URL `GET`s. Shadow work runs after the response is sent with a
10 s budget per phase and the concurrency cap above; event streams,
content-encoded bodies, and bodies over 8 MiB skip.

Comparison: status, content-type, body. Byte-identical bodies match;
otherwise JSON bodies are normalized over the fixed volatile allowlist
and compared semantically — request ids (`request_id`, `requestId`) and
presigned-URL query parameters (`X-Amz-Date`, `X-Amz-Expires`,
`X-Amz-Signature`, `Expires`, `Signature`, `sig`). Everything else must
match byte for byte. Extend the allowlist only with cited evidence.

On mismatch the proxy logs one structured `shadow mismatch` line (route
template, method, both statuses, a body excerpt capped at 2 KB with
tokens/cookies/authorization redacted, never the full body) and counts it.
Excerpts stay in the logs: the internal endpoint serves counters plus
route/method/status tuples only, and only to callers presenting the
metrics token (without it the endpoint 404s exactly as if it did not
exist, which is what Django serves for `/internal/*`):

```sh
curl -H "x-shadow-metrics-token: $PIDASH_RUST_SHADOW_METRICS_TOKEN" \
  localhost:8080/internal/shadow-metrics | jq .
# {"enabled": true,
#  "totals": {"compared": N, "matched": N, "mismatched": N,
#             "skipped": N, "errored": N},
#  "routes": {"/": {"compared": N, ...}, "_other": {...}},
#  "recent_mismatches": [{"route": ..., "method": ..., "django_status": ...,
#                         "rust_status": ...}]}
```

Counter keys come from a fixed template list — denylist templates,
exact static paths (`/`, `/robots.txt`), and the `_other` overflow
bucket for everything else — so raw request paths (slugs, uids, tokens)
never become keys. Keys are capped at 256 characters and 4096 entries,
so the map pins about a megabyte at most. Known limits: the shadow
dispatch shares the process (pools, throttles) and skips outer middleware
except a read-only session layer, so throttle-quota and host-absolute
responses can false-mismatch under load; `X-Api-Key` coverage needs a
read-only shadow auth mode (not built).

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

## Overlay surface (OVL-02)

The cloud overlay depends only on `pidash-sdk` (`crates/sdk`): a logic-free
facade of `pub use` re-exports grouped into `startup`, `state`, `domain`,
`read`, `ops`, `compat`, `assistant`, `jobs`, `db` and `testkit`. Every other
OSS path is internal and may change without notice — including paths that are
technically `pub` in their home crate.

Seeded with every OSS path the overlay imports (OVL-02, PIDASHCONV-1540);
OVL-03 (PIDASHCONV-1541)..OVL-07 (PIDASHCONV-1545) add their new public items
here as they land. The surface is snapshotted in `crates/sdk/SURFACE.txt` and
enforced by `crates/sdk/tests/surface.rs`: removing or renaming an export
fails the test, so the change shows up as a reviewed diff instead of a
surprise overlay breakage. Add an export by editing `src/lib.rs`,
`tests/surface.rs` and `SURFACE.txt` together (see the crate docs).

## Extension seams (F-10)

The six `ee/` stubs are traits with CE defaults; the private crate
replaces the implementation, never the callers:

| Trait | Module | CE default | Python source |
|---|---|---|---|
| `AssistantModelSeam` | `pidash-services::extensions` | `CeAssistantSeam` (`pidash-api::assistant::model_seam`; carried on `AppState`, install an alternate with `with_assistant_seam`) | `ee/assistant/model_provider.py` |
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

### Startup builders (OVL-03)

`pidash-api::startup` owns the serve and worker boot sequences, so the
overlay composes them instead of re-doing each step by hand (the hand
copies drifted three ways before OVL-03: a double-registered sweep, a
`sleep` where OSS drains under `timeout`, and a differently-opened
Redis). The OSS binary's `serve` and `worker` are thin callers of the
same builders:

```rust
// serve: resolve, pool, schema, Redis, ORM, state, app, bind, graceful stop.
pidash_api::startup::ServeBuilder::from_env()?
    .with_bind(addr)
    .run()
    .await?;
// worker: pools, schema, registry, turn handler, loops, bounded drain.
pidash_api::startup::WorkerBuilder::from_env()
    .await?
    .with_concurrency(4)
    .run()
    .await?;
```

`ServeBuilder` extension points: `with_settings`, the four seams above
(`with_toolsets`, `with_desktop_gate`, `with_assistant_seam`,
`with_user_settings_schema`), `with_state` for other state tweaks,
`merge_routes` (rides the overlay `add_routes` fallback path, so an
extra route shadows rather than colliding), `wrap` (outside the OSS
middleware stack), `wrap_inside` (inside the stack, after
`SessionLayer`, before the route handler — the overlay's JWT,
federation-guard and quota layers sit here; repeated calls apply in
call order, tower outside-in, so the JWT → federation → quota request
order reads quota, federation, JWT), `with_bind`/`with_version`, and
`run`. `build` returns the pool-less state-plus-router pair for tests
and embedding.

`WorkerBuilder` extension points: `with_settings` (mirrors serve's;
the final settings feed `ctx.settings`, the turn soft limit, and the
still-default seams, which rebuild — call before `with_turn_seam`),
`with_assistant_seam`, `with_turn_seam` (the builder is generic over
the non-object-safe `TurnSeam`; the default is the live OSS seam),
`register` for extra jobs (the closure gets the registry plus the
built `OssWorkerCtx`), `schedule` (appends after `default_schedule`),
`with_concurrency`, and `run`. `build` returns the registry plus
context without driving the loops. An extra registration over an
already-owned task name — OSS or an earlier extra — fails the build
instead of silently replacing the handler.

Both builders run the queue-schema DDL on an optional schema-owner
pool (`with_queue_schema_owner`, PIDASHCONV-1558): split-role cloud
deploys pass the migrator's pool — the app role is DML-only and
Postgres checks `CREATE` on the schema even for
`CREATE TABLE IF NOT EXISTS`. Unset, both run on the app pool exactly
as before (this closes the OVL-03 known gap the overlay port waited
on; no `pidash-sdk` surface change — the three hooks add methods, no
new public item types).

Both builders re-export through `pidash-sdk` as `startup` (OVL-02).

## Domain write services (ORM-02)

`pidash_services::domain` is the one public surface for writes to the
covered tables (design `.ai_design/rust_orm/design.md` D3, §4). Overlays
call it; they never execute OSS SQL text themselves:

```rust
pidash_services::domain::run(&ctx, &db, |txn| {
    Box::pin(async move {
        txn.advisory_xact_lock(key).await?;
        txn.on_commit(|| async move { /* post-commit effects */ });
        Ok::<(), pidash_services::domain::DomainError>(())
    })
})
.await?;
```

- `WriteCtx` — actor (the `crum`-user equivalent), request origin/host,
  workspace/project scope, and a per-caller options struct for the
  view-to-view deltas (D6).
- `WriteTxn` — the SeaORM transaction plus the after-commit queue and the
  hook registry. Entity calls run on it directly; existing sea-query
  builders go through `exec_builder`, raw SQL (advisory locks,
  `SELECT … FOR UPDATE`) through `exec_raw`. A SeaORM transaction cannot
  adopt an `sqlx::Transaction`, so domain writes never mix in sqlx
  transactions (D5). Reads stay on sqlx.
- After-commit queue — the `pidash_db::tx::AfterCommit` semantics
  (in order, exactly once, after a successful commit; errors roll back
  and drop the queue) with async steps, awaited by `run()` after the
  commit. Django timing correspondence: pi-dash sets no
  `ATOMIC_REQUESTS` (autocommit default), so its activity enqueues are
  plain inline `.delay()` calls and its `on_commit` callbacks run
  immediately outside `atomic()`; `domain::run` always opens a
  transaction instead, and no migrated path queues work yet, so moving a
  future enqueue into the queue changes neither the side effects nor
  their client-visible timing — each migration re-confirms this for its
  own paths. Failure modes, order and the outermost-only scope are
  documented with Django file citations on `CommitQueue`/`WriteTxn`.
- Hook registry — trait objects implemented wherever their dependencies
  live, so entity-adjacent logic can trigger services/jobs effects
  (scheduler seeding, the issue transition) without a crate cycle.
- `domain::activity` — the single copy of the `issue_activity`,
  `model_activity` and version task names; every previous copy is a
  re-export.
- Issue post-save orchestration — the pre-save snapshot lives in
  `domain::transition`, the completion comment and the transition fire
  core in `pidash_jobs::issue_transition`; the api module keeps
  same-path wrappers over its own drains.

## SeaORM entities (ORM-01)

SeaORM 1.1 (`~1.1.20`, design `.ai_design/rust_orm/design.md` D1) covers
the §4 tables only. The entities live at `crates/db/src/entity/`; Django
stays the schema owner — there are no SeaORM migrations.

One pool, two handles (D5): the `DatabaseConnection` wraps the existing
primary sqlx pool and opens no connections of its own. The binary builds
it once per process:

```rust
let orm = pidash_db::sea_orm_connection(pools.primary());
```

It travels on the request and worker contexts next to the pools:

- HTTP: `AppState::with_orm(orm)` at serve boot, read with
  `state.orm()` (`Option<&DatabaseConnection>`, `None` until the binary
  connects, like `pools()`). The shadow clone shares the handle.
- Worker: the `orm` field of `pidash_jobs::OssWorkerCtx`, read with
  `ctx.orm()`. The overlay binary sets it the same way when it builds
  its context for `register_oss_handlers`.

Regenerating after a Django sync (the `entity` module doc records the
exact command and Django SHA of the last generation):

```sh
export DATABASE_URL=postgresql://<user>@127.0.0.1:<port>/<db-migrated-by-apps/api>
sea-orm-cli generate entity --database-url "$DATABASE_URL" \
  --output-dir <staging-dir> \
  --tables issues,issue_sequences,issue_assignees,issue_comments,descriptions,\
issue_relations,users,profiles,accounts,user_notification_preferences,workspaces,\
workspace_members,projects,project_members,states,api_tokens,machine_token,\
runner_session,assistant_user_llm_config,assistant_message
```

Then copy the 22 files over `crates/db/src/entity/` and re-apply the
four mechanical cleanups (the generator rewrites them): delete the
`belongs_to` relations (plus `Related` impls) pointing at tables outside
the §4 set — the compiler lists them as unresolved `super::<table>`
paths; restore the module doc on `mod.rs` with the new Django SHA;
restore the `#[cfg(test)] mod parity;` declaration; and delete the empty
`impl ActiveModelBehavior for ActiveModel {}` the generator restores in
`users.rs` (the hand-written impl lives in `users_hooks.rs`, which the
generator never touches — keeping both fails the build with duplicate
impls, loud by design). Finish with `cargo fmt`, the `entity::parity`
tests below, and the usual clippy run.

Schema parity: `crates/db/src/entity/parity.rs` compares every entity's
table name, column names, column types and nullability with
`information_schema.columns` on the migrated database, and fails on any
drift (missing/extra column, renamed table, type or nullability change).
Like every live-db test it skips when `DATABASE_URL` is unset and runs
for real otherwise:

```sh
export DATABASE_URL=postgresql://<user>@127.0.0.1:<port>/<migrated-db>
cargo test -p pidash-db entity::parity
```

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

## Ops image + runbook (D-37)

The Rust container layer (PIDASHCONV-813): `rust-api/Dockerfile.api`,
`rust-api/bin/docker-entrypoint-*.sh`, and this runbook. It translates
`apps/api/Dockerfile.api` and `apps/api/bin/docker-entrypoint-*.sh`
(F37-12); the replay suite is `contract-tests/ops/test_image.py`.

### Image build / push

Build from the repo root (the context needs both `rust-api/` and
`apps/api/` — the binary embeds templates and fragments from `apps/api`
at compile time):

```sh
docker build -f rust-api/Dockerfile.api -t airepublic/pi-dash-backend:rust .
docker push airepublic/pi-dash-backend:rust
```

Multi-stage: `rust:bookworm` compiles `pidash-api --release`, then the
binary + entrypoints + data ship on `debian:bookworm-slim` (no toolchain,
no Python). Same contract as the Python image: `WORKDIR /code`,
`EXPOSE 8000`, `CMD ["./bin/docker-entrypoint-api.sh"]`. There is no
`.dockerignore` (quarantine: only `rust-api/` plus the workflow change),
so local builds send the whole checkout as context.

### Boot order

One image, seven entrypoints (role = container `command`, as in the
Python fleet). Every `python manage.py <cmd>` became
`pidash-api ops <cmd>` (`ops instance <cmd>` for the nested group);
`gunicorn`/`celery` exec lines became `serve` / `worker`:

| Entrypoint | Boot steps |
|---|---|
| `docker-entrypoint-api.sh` | wait_for_db → wait_for_migrations → machine signature → register-instance → configure-instance → create_bucket → clear_cache → `serve --bind 0.0.0.0:${PORT:-8000}` |
| `docker-entrypoint-api-local.sh` | same boot + `DJANGO_SETTINGS_MODULE` default → `serve --bind 0.0.0.0:8000` |
| `docker-entrypoint-worker.sh` | waits → min(nproc,8) concurrency → `worker --concurrency` |
| `docker-entrypoint-worker-local.sh` | waits → `worker` |
| `docker-entrypoint-beat.sh` | waits → `worker` (no `exec`, like Python) |
| `docker-entrypoint-beat-local.sh` | waits → `worker` |
| `docker-entrypoint-migrator.sh` | wait_for_db → `python manage.py migrate $1` (stays Django) |

Deltas from the Python boot (each documented at the script line):
collectstatic dropped (API-only container; the Django upstream serves
static); migrate stays `python` (schema owner until switchover) and the
migrator stays pinned to the Python image until the deployments
follow-up; GUNICORN_WORKERS and max-requests have no `serve` equivalent
(scale with replicas); watchmedo / uvicorn --reload dropped in -local
scripts (`cargo watch` outside the image); beat roles run the full worker
(no scheduler-only mode — the scheduler lock keeps worker + beat from
double-scheduling); the migrator's `wait_for_db $1` drops `$1` (Django
ignores it, clap rejects it).

### Environment

Every variable the image sets or the scripts and boot path consume:

| Variable | Default | Consumed by | Notes |
|---|---|---|---|
| `PORT` | `8000` | api entrypoint | `--bind 0.0.0.0:${PORT:-8000}`; serve alone defaults to 8080 |
| `CELERY_WORKER_CONCURRENCY` | min(nproc, 8) | worker entrypoint | job budget; Rust multiplexes over a shared 10-conn pool |
| `DJANGO_SETTINGS_MODULE` | `pi_dash.settings.local` | api-local entrypoint | no Rust effect; kept for shape parity |
| `MACHINE_SIGNATURE` | (set by api entrypoints) | register-instance | sha256 of hostname, mac, cpu, mem, disk |
| `DATABASE_URL` | (required) | serve, worker, ops | TCP postgres URL; replica: see below |
| `DATABASE_READ_REPLICA_URL` | (unset) | settings key only | read by Settings; pool wiring not yet connected |
| `SECRET_KEY` | (ephemeral if unset) | serve, ops | session validation plus Fernet; keep stable across restarts |
| `REDIS_URL` | (unset) | serve (degraded), clear_cache | throttle and signal cache; clear_cache full-clears |
| `AWS_REGION` | `AWS_DEFAULT_REGION`, then `us-east-1` | create and update bucket | region chain; set-wins even when empty |
| `AWS_DEFAULT_REGION` | (unset) | create and update bucket | second in the region chain |
| `AWS_ACCESS_KEY_ID` | (unset) | create and update bucket | S3 credentials |
| `AWS_SECRET_ACCESS_KEY` | (unset) | create and update bucket | S3 credentials |
| `AWS_S3_ENDPOINT_URL` | (unset) | create and update bucket | S3-compatible endpoint (MinIO, LocalStack) |
| `AWS_S3_BUCKET_NAME` | (unset) | create and update bucket | default bucket |
| `SEED_DIR` | (unset) | worker seed path, 811 loader | unset: worker reads ./seeds, loader serves embedded data |
| `PIDASH_RELEASES_URL` | GitHub releases API | register-instance | override for the latest-release probe (tests) |
| `INSTANCE_CHANGELOG_URL` | (Dockerfile default) | settings | preserved from the Python image |
| `PYTHONDONTWRITEBYTECODE` | `1` | (none) | preserved key; no interpreter in this image |
| `PYTHONUNBUFFERED` | `1` | (none) | preserved key; no interpreter in this image |
| `PIP_DISABLE_PIP_VERSION_CHECK` | `1` | (none) | preserved key; no interpreter in this image |

Cutover flags (`PIDASH_DJANGO_UPSTREAM`, `PIDASH_RUST_*`) live in
[Cutover edge](#cutover-edge-f-02); Django-side deploy vars
(`CELERY_BROKER_URL`, `GUNICORN_WORKERS`) stay in `deployments/`
(quarantine — see follow-ups). The Rust worker holds no broker config:
the job queue is Postgres (`rust_job_queue`) and there is no AMQP path
(PIDASHCONV-1108).

### Replica wiring (F37-11)

Django: `ENABLE_READ_REPLICA=1` enables the replica `DATABASES` entry
from `DATABASE_READ_REPLICA_URL` (else the `POSTGRES_READ_REPLICA_*`
parts) plus the `ReadReplicaRouter` and routing middleware; the F37-11
decision table routes GET/HEAD/OPTIONS with a truthy `use_read_replica`
view attribute to the replica and everything else to primary
(`rust-api/fixtures/ops/routing/decision_table.golden.json`, ported to
`crates/api/src/ops/routing.rs` by PIDASHCONV-812). Rust: the decision
mirror and the `DATABASE_READ_REPLICA_URL` settings key exist, but every
`Pools::connect` still passes `None` for the replica pool, so all reads
stay on primary until a follow-up wires the pool — no replica env needs
setting on this image today.

### Follow-ups (out of scope, quarantine)

- `deployments/{aio,cli,kubernetes,swarm}/`: point roles at this image
  (migrator stays on the Python image until migrate is ported).
- `requirements/*.txt`: drop the 3 unused deps plus croniter
  (post-switchover).
- Worker seed join: `services::tasks_cleanup::workspace_seed` joins
  `dir/filename` without Python's `data/` segment; align it to
  `SEED_DIR/data` and move the image seeds to `./seeds/data/`.
- Replica pool wiring: connect `DATABASE_READ_REPLICA_URL` in
  serve/worker instead of `None`.

## Reference

- PIDASHCONV-1: the rulebook (rules, stages, issue types, where things live).
- Porting guide wiki page: crate layout, Django idiom → Rust pattern table,
  semantic traps. Foundation issues and pilots update it.
