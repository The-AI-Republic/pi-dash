# Write-replay validation (PIDASHCONV-823)

Replays ordered write requests against Django and Rust (all cutover
flags on) on two copies of one seeded snapshot, comparing status +
body, database state, enqueued jobs, and sink deliveries after every
request. Writes cannot be shadowed on a live database; replay them
instead against two copies of the same data.

## Topology

Three servers, no workers, no beat (enqueued jobs accumulate
observably instead of executing):

| server | serves | database | broker (publishes) | SMTP sink |
|---|---|---|---|---|
| Django-direct | `REPLAY_DJANGO_URL` | `REPLAY_DJANGO_DB` | `CELERY_BROKER_URL` (Redis) | `REPLAY_DJANGO_SMTP_PORT` |
| Django-upstream | (Rust proxy only) | `REPLAY_RUST_DB` | `REPLAY_UPSTREAM_BROKER_URL` (Redis) | `REPLAY_RUST_SMTP_PORT` |
| Rust, all `PIDASH_RUST_*=1` | `REPLAY_RUST_URL` | `REPLAY_RUST_DB` | — (jobs land in `rust_job_queue`) | — (serve sends no mail) |

Each Django gets its own Redis cache DB (`REDIS_URL`) and broker DB so
throttle counters and published jobs never leak across backends; Rust
gets its own cache DB too. Webhook rows are seeded with a placeholder
host and rewritten per backend at restore time
(`REPLAY_DJANGO_WEBHOOK_BASE` / `REPLAY_RUST_WEBHOOK_BASE`). All three
servers share one `SECRET_KEY` (forged sessions must verify on both).

Mail and webhook deliveries are worker-driven on both backends (views
enqueue; the worker delivers), so with no workers running the sinks
normally stay empty on both sides — the comparison still runs, and any
synchronous delivery on either side fails loudly.

## Run locally

```sh
cd rust-api/contract-tests
# 1. Migrated template database + world seed + snapshot (once):
psql postgres -c "CREATE DATABASE replay_template;"
cd ../../apps/api
SECRET_KEY=replay-secret DATABASE_URL="postgresql://.../replay_template" \
  WEB_URL=http://127.0.0.1:18223 APP_BASE_URL=http://127.0.0.1:18223 \
  python manage.py migrate --no-input
cd ../../rust-api/contract-tests
python -m replay.snapshot seed --database-url "postgresql://.../replay_template"
python -m replay.snapshot dump --database-url "postgresql://.../replay_template" \
  --out /tmp/replay-snapshot.sql
# 2. Boot the three servers (see .github/workflows/rust-api-replay.yml
#    for the full env; ports/sinks must match the REPLAY_* vars below).
# 3. Run:
export REPLAY_DJANGO_URL=http://127.0.0.1:18223
export REPLAY_DJANGO_DB="postgresql://.../replay_django"
export REPLAY_RUST_URL=http://127.0.0.1:18225
export REPLAY_RUST_DB="postgresql://.../replay_rust"
export REPLAY_UPSTREAM_BROKER_URL=redis://127.0.0.1:6379/12
export CELERY_BROKER_URL=redis://127.0.0.1:6379/11
export REPLAY_DJANGO_REDIS_URL=redis://127.0.0.1:6379/13
export REPLAY_RUST_REDIS_URL=redis://127.0.0.1:6379/15
export REPLAY_RUST_AMQP_URL=amqp://pi_dash:pi_dash@127.0.0.1:5672/replay
export SECRET_KEY=replay-secret
export REPLAY_SNAPSHOT=/tmp/replay-snapshot.sql
export REPLAY_DJANGO_WEBHOOK_BASE=http://127.0.0.1:18131
export REPLAY_RUST_WEBHOOK_BASE=http://127.0.0.1:18132
export REPLAY_DJANGO_SMTP_PORT=11231
export REPLAY_RUST_SMTP_PORT=11232
pytest replay/test_replay.py replay/test_meta.py -v
```

`test_meta.py` needs no backends (drift guards + manifest validation).
`test_replay.py` skips cleanly without `REPLAY_DJANGO_URL`.

Databases `replay_django` / `replay_rust` must exist and be migrated
(the runner only truncates + loads data, never DDL — except the Rust
queue tables, which it ensures in the Rust copy).

`REPLAY_DJANGO_REDIS_URL` / `REPLAY_RUST_REDIS_URL` are the two servers'
cache-Redis URLs (their `REDIS_URL`, not the brokers). The runner clears
DRF throttle counters and `magic_*` rows on both before every session,
so per-IP rate limits and code attempt counters never leak between
sessions; a corpus session's `setup` hook runs after the flush and may
prime same-value Redis state (magic-link codes) on both.

`REPLAY_RUST_AMQP_URL` is the Rust server's AMQP broker (its `AMQP_URL`,
a RabbitMQ the topology provides). Most Rust handlers enqueue into
`rust_job_queue`, but a few publish direct to AMQP
(`auth_session/password.rs` forgot-password, `space/assets.rs`, ...);
without this broker those handlers 500, and the runner drains it per
request as Rust-side jobs.

## Coverage

```sh
python rust-api/contract-tests/replay/coverage.py check --binary <pidash-api>
```

fails on any write `(route, method)` pair that is neither covered by a
session nor listed with a reason in `corpus/reasons.py`, and on stale
reasons. `coverage.py write` regenerates `COVERAGE.md`.

## Adding a session

1. Read the domain suite under `rust-api/contract-tests/<domain>/` for
   valid payloads and statuses.
2. Add `corpus/<domain>.py` with `SESSIONS` (see `corpus/app_issues.py`
   for the shape), register it in `corpus/__init__.py`, and cite the
   suite files in `Session.source`.
3. World placeholders (`{ws}`, `{project}`, `{admin}`, ...) come from
   `corpus/world.py`; per-backend generated ids come from `capture`
   pointers (`issue/id`, `@cookie:session-id`, `@header:Location`).
4. Declare the route template on every write request (coverage joins on
   shape). `expect_status` pins Django's answer so corpus bugs fail fast.
5. Delete the session's rows from `corpus/reasons.py` (`OWNED_UNCOVERED`)
   and regenerate `COVERAGE.md`.

Comparison tuning (`timestamp_columns`, `generated_columns`,
`skip_tables`, `payload_time_paths`) names exact tables/columns/paths —
never patterns. A loud false diff is fixed with a precise entry plus
cited evidence, here or in `effects.py`, never with a looser rule.
