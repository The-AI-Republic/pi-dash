"""Shared black-box helpers for Rust contract suites.

Every suite runs against a live server (Django today, Rust via the proxy
tomorrow) driven by BASE_URL, and seeds rows straight into Postgres via
DATABASE_URL. Nothing here imports Django or touches its test client.
"""

# Shared contract-test helpers. Extend this package; never fork per-domain copies.
# Shared contract-test harness (first use: PIDASHCONV-19, integrations domain).
# Extend, never fork: new domains add helpers here, not copies.
"""Black-box helpers for Celery-task contract tests.

Nothing here imports Django. Tests talk to the system under test only via:
  * CELERY_BROKER_URL — publish jobs in Celery protocol-v2 wire format,
    observe queue depth and worker-published follow-ups;
  * DATABASE_URL — seed rows straight into Postgres, snapshot/diff tables;
  * BASE_URL — drive the live Django HTTP API where a DB write must pass
    through ORM signals (e.g. github_signals completion hook);
  * local stub sinks (sinks.py) — capture outbound side effects.
"""
