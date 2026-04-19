# Runner — Operator / Admin Guide

This covers the cloud-side operation of the runner subsystem: deployment, capacity, monitoring, and incident response. For user-facing onboarding see `user-guide.md`.

## What runs where

| Component                            | Owner       | Host                       | Resources                                            |
| ------------------------------------ | ----------- | -------------------------- | ---------------------------------------------------- |
| Django ASGI (includes `/ws/runner/`) | `apps/api`  | existing `api` service     | +1 worker recommended per 1k concurrent runners      |
| `runner.expire_stale_approvals`      | Celery beat | existing `beat` + `worker` | 1 invocation/min                                     |
| `runner.mark_offline_runners`        | Celery beat | existing `beat` + `worker` | 1 invocation/min                                     |
| Redis / Valkey                       | existing    | existing                   | Channels needs pub/sub; re-uses existing `REDIS_URL` |
| Postgres                             | existing    | existing                   | +4 small tables (see below)                          |

No new deploy target, no new secret. If `REDIS_URL` is set (production), Channels uses `channels_redis.core.RedisChannelLayer`; otherwise (dev/test) `channels.layers.InMemoryChannelLayer` (single-process only).

## Database impact

Four new tables, all prefixed with `runner_` or `agent_run_`:

| Table                       | Growth pattern                         | Retention                         |
| --------------------------- | -------------------------------------- | --------------------------------- |
| `runner`                    | O(users × 5)                           | Keep revoked rows for audit       |
| `runner_registration_token` | Short-lived; consumed or expires in 1h | Safe to GC after 30 days          |
| `agent_run`                 | One per user-triggered run             | Keep forever by default           |
| `agent_run_event`           | N per run, up to 100s                  | Consider S3 archive after 30 days |
| `agent_run_approval`        | ≤1 per approval request                | Keep forever (audit)              |

Nothing here is bounded by the existing Postgres tuning. The largest table by row count will be `agent_run_event`; plan a rollup if you expect >1M runs/month.

## Configuration

| Env var     | Default          | Meaning                                         |
| ----------- | ---------------- | ----------------------------------------------- |
| `REDIS_URL` | required in prod | Used by cache, channel layer, existing services |
| `AMQP_URL`  | required in prod | Celery broker                                   |

No new env vars. Runner caps are code constants:

- `Runner.MAX_PER_USER = 5` (`models.py`) — per-user runner cap
- `HEARTBEAT_INTERVAL_SECS = 25` (`consumers.py`) — advertised to the daemon in `welcome`
- `HEARTBEAT_OFFLINE_GRACE = 90s` (`tasks.py`) — how long before a stale-heartbeat runner is demoted to offline
- approval expiry = 10 minutes (`supervisor.rs` sets it; `expire_stale_approvals` enforces it)

Bump these in a deploy if the defaults turn out wrong for your workload. Log first (see `observability.md`), then raise.

## Reverse-proxy

The Caddy configs (`apps/proxy/Caddyfile.ce`, `Caddyfile.aio.ce`) already route `/ws/runner/*` to `api:8000`. If you run your own proxy, the WS upgrade needs:

- `Connection: upgrade` and `Upgrade: websocket` passed through (Caddy does this by default on `reverse_proxy`)
- no body-size limit on the upgrade hop
- the `Authorization: Bearer ...` header preserved

## Monitoring

Scrape `GET /api/v1/runner/metrics/` — see `observability.md` for the full schema. Suggested dashboard:

1. Current online / busy / offline runner counts
2. Active runs (should oscillate around your workspace's typical concurrency)
3. Pending approvals (should near zero; sustained growth = user workflow blocker)
4. Offline burst (delta of `offline` over 5m) — indicator of a deploy or network incident

The runner consumer logs structured events under the `pi_dash.runner.consumers` logger. Surface `WARN`/`ERROR` from that logger in your aggregation tool.

## Incident response

### Symptom: no runners ever come online

1. `GET /api/v1/runner/health/` — returns `{"ok": true}`? If not, the Django/ASGI process is unhealthy.
2. `GET /api/v1/runner/metrics/` — returns counts at all? If the response is empty or 5xx, the DB is unreachable.
3. On the runner side, `pi-dash-runner status --json` — does the daemon see the WS as connected?
4. Check Caddy / your proxy access logs for `/ws/runner/` upgrade requests. 426 or 400 indicates the upgrade headers are being stripped.

### Symptom: approvals pile up and never get decided

1. Check `pi_dash_approvals_pending` — is it climbing or stable?
2. Is `runner.expire_stale_approvals` running? `celery -A pi_dash.celery beat-status`. It should fire once per minute.
3. If the scheduler is off, an approval older than 10 minutes should have been auto-cancelled on the next task tick.

### Symptom: thundering-herd reconnect after a deploy

Runners use jittered exponential backoff capped at 60s, but if many runners disconnect at once you'll see a reconnect window of ~1–60s on rolling restart. This is expected. If a flap persists, check:

- Caddy / LB is not enforcing idle timeouts shorter than 30s (would kick all heartbeating runners)
- Django ASGI worker count is sufficient for the connection load

### Symptom: a user hits the 5-runner cap

Rare by design. If legitimate (e.g. a user has 5 dev machines plus a build box), bump `Runner.MAX_PER_USER` — but see whether they're leaking revoked rows first. Revoked runners do NOT count against the cap, but stale "online" rows from machines that never cleanly disconnected do until the `mark_offline_runners` Celery task runs.

## Backup / disaster recovery

The runner sub-schema is covered by whatever Postgres backup regime you run for the rest of Pi Dash. Redis state is not restored from backup — runners will reconnect and repopulate their `runner.<id>` Channels groups automatically.

Per-runner transcripts (`history/runs/*.jsonl`) live on the user's laptop only. If a user loses their machine, those are gone. This is a design choice (local-only events).

## Upgrades

Bump `PROTOCOL_VERSION` in `pi_dash/runner/consumers.py` **and** `runner/src/lib.rs` together. Runner daemons on an older protocol will log a warning but continue; the server can be stricter by rejecting mismatched versions at `hello` (commented path in `on_hello`).
