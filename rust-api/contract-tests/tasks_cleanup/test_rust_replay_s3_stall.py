# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-09 stall oracle, Rust-side replay (PIDASHCONV-1291).

Executable form of ``test_s3_stall.py`` against the Rust worker: the
``rust_job_queue`` row carries the exact Celery v2 payload (see
``_harness/rust_queue.py``) and the assertions are the same DB
before/after checks plus the Rust-observable failure state.

Stall environment only: the Rust worker under test boots with
``AWS_S3_ENDPOINT_URL=http://127.0.0.1:<STALL_S3_PORT>`` (default 4599),
``USE_MINIO=1``, and ``--concurrency 1``. This module runs alone in its
own CI job (see ``rust-api-contract-tests-tasks-cleanup-stall.yml``);
the shared replay job pins the live-S3 file only, and the Django
worker-plane job ignores ``test_rust_replay*.py`` by glob.

Expected (Django behavior, botocore defaults): the S3 call times out
after 60s and the task fails and ends. The Rust deleter attempts every
delete once (no botocore-style retries — a documented deviation in
``expiry.rs``), so the job must park ``failed`` at about 60s. Before the
fix the job hangs forever and this test times out at its bound.

Local run (own scratch database, Django worker STOPPED)::

    export DATABASE_URL=postgresql://... STALL_S3_PORT=4599
    export AWS_S3_ENDPOINT_URL=http://127.0.0.1:4599 USE_MINIO=1
    pidash-api worker --concurrency 1 &  # Rust worker; Django worker stopped
    pytest -p no:django tasks_cleanup/test_rust_replay_s3_stall.py
"""

import time
import uuid
from datetime import datetime, timedelta, timezone

from _harness import rust_queue, stall
from _harness import seed as seed_helpers
from _harness.db import insert_row, wait_for_condition as wait_for

M = "pi_dash.bgtasks"

# Single 60s read-timeout attempt (no retries on the Rust side); the
# bound leaves headroom for loaded CI without letting a hang pass.
STALL_BOUND_SECS = 180
# Any finish faster than the 60s timeout means the S3 call never stalled
# (misconfigured endpoint, fast refusal) — the test must fail, not pass.
MIN_STALL_SECS = 50


def _old(days: int = 60) -> str:
    return (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()


def _missing(conn, table: str, pk) -> bool:
    with conn.cursor() as cur:
        cur.execute(f'SELECT 1 FROM "{table}" WHERE id = %s::uuid', (str(pk),))
        return cur.fetchone() is None


def _column_is_null(conn, table: str, pk, column: str) -> bool:
    with conn.cursor() as cur:
        cur.execute(f'SELECT {column} FROM "{table}" WHERE id = %s::uuid', (str(pk),))
        row = cur.fetchone()
        return row is not None and row[0] is None


def _job_state(conn, celery_id: str):
    with conn.cursor() as cur:
        cur.execute(
            "SELECT status, last_error FROM rust_job_queue WHERE celery_id = %s",
            (celery_id,),
        )
        return cur.fetchone()


def test_rust_delete_old_s3_link_stall_fails_bounded(db_conn):
    """A wedged S3 fails the Rust expiry job within a bounded time.

    The worker dials a black hole (accept, never reply). The delete must
    hit the 60s read bound and park the job ``failed`` instead of holding
    the worker slot; a sentinel ``delete_api_logs`` job published right
    after proves the slot is free again, and the expired row's ``url``
    must still be set (fail-stop, like Django).
    """
    owner = seed_helpers.user(db_conn, "rust-replay-stall-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplaystall", owner["id"])
    row = insert_row(
        db_conn,
        "exporters",
        {
            "workspace_id": str(workspace["id"]),
            "type": "issue_exports",
            "provider": "csv",
            "status": "completed",
            "reason": "",
            "url": "https://example.com/rust-stall-expired.zip",
            "key": f"rust-replay-stall-{uuid.uuid4().hex}.zip",
            "token": f"rust-replay-{uuid.uuid4().hex}",
            "initiated_by_id": str(owner["id"]),
            "created_at": _old(days=9),
        },
    )
    sentinel = insert_row(
        db_conn,
        "api_activity_logs",
        {
            "token_identifier": "rust-replay-stall-probe",
            "path": "/rust-replay/stall-old",
            "method": "GET",
            "response_code": 200,
            "created_at": _old(),
        },
    )
    with stall.black_hole() as listener:
        started = time.monotonic()
        stall_id = rust_queue.publish(f"{M}.exporter_expired_task.delete_old_s3_link")
        rust_queue.publish(f"{M}.cleanup_task.delete_api_logs")
        wait_for(
            lambda: _missing(db_conn, "api_activity_logs", sentinel["id"]),
            timeout=STALL_BOUND_SECS,
            what=f"sentinel api_activity_logs row deleted after stalled expiry (bound {STALL_BOUND_SECS}s)",
        )
        elapsed = time.monotonic() - started
        state = _job_state(db_conn, stall_id)
    assert listener.hits >= 1, "worker never dialed the stall endpoint"
    assert elapsed >= MIN_STALL_SECS, f"finished in {elapsed:.1f}s without stalling?"
    assert state is not None, "stall job row vanished instead of parking failed"
    assert state[0] == "failed", f"stall job status is {state[0]!r}, expected 'failed'"
    assert "transport failure" in (state[1] or ""), f"unexpected last_error: {state[1]!r}"
    assert not _column_is_null(
        db_conn, "exporters", row["id"], "url"
    ), "expired url cleared despite the S3 failure"
