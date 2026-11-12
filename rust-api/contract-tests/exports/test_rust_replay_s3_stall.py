# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stall oracle, Rust-side replay (PIDASHCONV-1727).

Executable form of ``test_s3_stall.py`` against the Rust worker: the
``rust_job_queue`` row carries the exact Celery v2 payload (see
``_harness/rust_queue.py``) and the assertions are the same DB
before/after checks plus the Rust-observable failure state.

Stall environment only: the Rust worker under test boots with
``AWS_S3_ENDPOINT_URL=http://127.0.0.1:<STALL_S3_PORT>`` (default
4599), ``USE_MINIO=1``, and ``--concurrency 1``. This module runs
alone in its own CI job (see
``rust-api-contract-tests-exports-stall.yml``); the shared replay
job points at live S3, and the Django worker-plane job ignores
``test_rust_replay*.py`` by glob.

Expected (Django behavior, botocore defaults): the S3 call times out
after 60s per attempt and is retried up to five attempts; once the
retries are exhausted the exporter row becomes ``failed``. The Rust
upload carries the same 60s connect / 60s read bounds plus the
legacy retry loop, so the job must end failed at about 5x60s plus
backoff. Before the fix the job hangs forever and this test times
out at its bound.

Local run (own scratch database, Django worker STOPPED)::

    export DATABASE_URL=postgresql://... STALL_S3_PORT=4599
    export AWS_S3_ENDPOINT_URL=http://127.0.0.1:4599 USE_MINIO=1
    pidash-api worker --concurrency 1 &  # Rust worker; Django worker stopped
    pytest -p no:django exports/test_rust_replay_s3_stall.py
"""

import time
from datetime import datetime, timedelta, timezone

from _harness import rust_queue, stall
from _harness.db import insert_row, wait_for_condition as wait_for
from _harness.seed import Seed

from . import cases

SENTINEL_TASK = "pi_dash.bgtasks.cleanup_task.delete_api_logs"

# 5 botocore attempts x 60s read timeout plus backoff is about 315s;
# the bound leaves headroom for loaded CI without letting a hang pass.
STALL_BOUND_SECS = 480
# Any finish faster than one 60s timeout means the S3 call never stalled
# (misconfigured endpoint, fast refusal) -- the test must fail, not pass.
MIN_STALL_SECS = 50


def _old(days: int = 60) -> str:
    return (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()


def _sentinel_gone(db_conn, sentinel_id) -> bool:
    with db_conn.cursor() as cur:
        cur.execute(
            'SELECT 1 FROM "api_activity_logs" WHERE id = %s::uuid',
            (str(sentinel_id),),
        )
        return cur.fetchone() is None


def _wait_exporter(token: str, db_conn, what: str) -> dict:
    def settled():
        row = cases.read_exporter(db_conn, token)
        return row if row["status"] in ("completed", "failed") else None

    return wait_for(settled, what=what, timeout=STALL_BOUND_SECS)


def test_rust_issue_export_stall_fails_bounded(db_conn):
    """A wedged S3 fails the Rust issue export within a bounded time.

    The worker dials a black hole (accept, never reply). The upload
    must hit the 60s bounds and exhaust the legacy attempts, ending
    with the exporter row ``failed`` instead of holding the worker
    slot forever; a sentinel ``delete_api_logs`` job published right
    after proves the slot is free again.
    """
    seed = Seed(db_conn)
    try:
        world = cases.seed_export_world(seed, cases.unique_tag("stall"))
        token = cases.unique_tag("tok")
        cases.seed_exporter(
            seed,
            workspace_id=world["workspace"],
            provider="csv",
            token=token,
            initiated_by_id=world["owner"],
        )
        sentinel = insert_row(
            db_conn,
            "api_activity_logs",
            {
                "token_identifier": "rust-replay-export-stall-probe",
                "path": "/rust-replay/export-stall-old",
                "method": "GET",
                "response_code": 200,
                "created_at": _old(),
            },
        )
        with stall.black_hole() as listener:
            started = time.monotonic()
            rust_queue.publish(
                cases.ISSUE_EXPORT_TASK,
                kwargs={
                    "provider": "csv",
                    "workspace_id": world["workspace"],
                    "project_ids": [world["project"]],
                    "token_id": token,
                    "multiple": False,
                    "slug": world["slug"],
                },
            )
            rust_queue.publish(SENTINEL_TASK)
            row = _wait_exporter(token, db_conn, "rust stalled issue export failed")
            wait_for(
                lambda: _sentinel_gone(db_conn, sentinel["id"]),
                timeout=120,
                what="sentinel api_activity_logs row deleted after stalled export",
            )
            elapsed = time.monotonic() - started
        assert listener.hits >= 1, "worker never dialed the stall endpoint"
        assert elapsed >= MIN_STALL_SECS, f"finished in {elapsed:.1f}s without stalling?"
        assert row["status"] == "failed", row
        assert row["reason"], row
        assert row["url"] is None, row
    finally:
        cases.cleanup_export_world(seed)
        seed.cleanup()
