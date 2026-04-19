# Runner Observability

## Metrics (Prometheus)

`GET /api/v1/runner/metrics/` exposes point-in-time gauges in the Prometheus
text exposition format (v0.0.4). Scrape every 15–60s.

| Metric                            | Type  | Meaning                                                                        |
| --------------------------------- | ----- | ------------------------------------------------------------------------------ |
| `pi_dash_runner_online`     | gauge | Runners currently `online` in the DB.                                          |
| `pi_dash_runner_busy`       | gauge | Runners currently executing a run.                                             |
| `pi_dash_runner_offline`    | gauge | Runners whose heartbeat has lapsed. Excludes revoked.                          |
| `pi_dash_runs_active`       | gauge | AgentRuns in `assigned` / `running` / `awaiting_approval` / `awaiting_reauth`. |
| `pi_dash_approvals_pending` | gauge | ApprovalRequests with `status=pending`.                                        |

### Alert rules (starter)

```yaml
- alert: RunnerOfflineBurst
  expr: delta(pi_dash_runner_offline[5m]) > 5
  for: 5m
  annotations:
    summary: "More than 5 runners went offline in 5 minutes — check WS service."

- alert: ApprovalBacklog
  expr: pi_dash_approvals_pending > 20
  for: 15m
  annotations:
    summary: "Approval backlog >20 for 15 minutes — user workflow is blocked."
```

## Log schema

Every runner subsystem emits structured records via `tracing` (runner) and
Python `logging` (Django). Use these fields consistently so log search works.

### Runner daemon (`pi_dash_runner`, Rust `tracing`)

| Field              | Type   | Emitted on                             |
| ------------------ | ------ | -------------------------------------- |
| `runner_id`        | uuid   | always when available                  |
| `run_id`           | uuid   | any frame scoped to an AgentRun        |
| `approval_id`      | string | approval flow events                   |
| `protocol_version` | int    | on handshake                           |
| `cloud_url`        | string | on connection open/close               |
| `thread_id`        | string | Codex bridge events                    |
| `codex.stderr`     | target | drained stderr from `codex app-server` |

### Django consumer (`pi_dash.runner.consumers`)

| Event                      | Level | Fields                                            |
| -------------------------- | ----- | ------------------------------------------------- |
| Protocol mismatch at hello | WARN  | `runner_id`, `client_protocol`, `server_protocol` |
| Duplicate `mid` dropped    | DEBUG | `runner_id`, `mid`                                |
| Seq replay dropped         | INFO  | `runner_id`, `run_id`, `seq`, `last`              |
| Seq gap                    | INFO  | `runner_id`, `run_id`, `seq`, `last`              |
| Unknown message type       | DEBUG | `runner_id`, `type`                               |
| Exception in handler       | ERROR | `runner_id`, `type`, traceback                    |

### Celery tasks

| Task                            | Log on                     | Fields  |
| ------------------------------- | -------------------------- | ------- |
| `runner.expire_stale_approvals` | Count of expired approvals | `count` |
| `runner.mark_offline_runners`   | Count of runners demoted   | `count` |

## Runbook pointers

- Runner stuck `online` but no activity → scrape `/api/v1/runner/metrics/`;
  if `pi_dash_runs_active > 0` but specific runs are stale, check the
  Celery `mark_offline_runners` cadence and `last_heartbeat_at` on the row.
- Repeated reconnect storms → look for `protocol mismatch` lines at WARN and
  the Rust daemon's `cloud WS connect failed` at WARN. Jittered backoff caps
  at 60s per daemon; if the server side is flapping, the clients will
  exponentially back off.
