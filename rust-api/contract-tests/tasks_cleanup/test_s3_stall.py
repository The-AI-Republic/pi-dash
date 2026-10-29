# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-09 stall oracle: ``delete_old_s3_link`` against a wedged S3 (PIDASHCONV-1291).

Stall environment only: the Django worker under test boots with
``AWS_S3_ENDPOINT_URL=http://127.0.0.1:<STALL_S3_PORT>`` (default 4599),
``USE_MINIO=1``, and serial execution (``--pool=solo``). This module runs
alone in its own CI job (see
``rust-api-contract-tests-tasks-cleanup-stall.yml``); the shared
worker-plane job ignores it — its worker points at live S3, and a live
endpoint would answer the delete instead of stalling.

Django behavior (``exporter_expired_task.py:28-51``): the boto3 client
carries no timeout ``Config``, so botocore's defaults apply (60s connect
/ 60s read, plus retries) — the task fails after about 5x60s and the
row's ``url`` is never cleared. The Rust port must bound its client the
same way (see ``test_rust_replay_s3_stall.py``); this module pins the
Django side of that contract and passes throughout.

Local run (own scratch database, Django worker only)::

    export DATABASE_URL=postgresql://... STALL_S3_PORT=4599
    export AWS_S3_ENDPOINT_URL=http://127.0.0.1:4599 USE_MINIO=1
    export CELERY_BROKER_URL=redis://... # same broker the worker uses
    celery -A pi_dash worker -E --pool=solo -I <oracle includes> &
    pytest -p no:django tasks_cleanup/test_s3_stall.py
"""

import time
import uuid
from datetime import datetime, timedelta, timezone

from _harness import celery_wire, stall
from _harness import seed as seed_helpers
from _harness.db import insert_row, wait_for_condition as wait_for

M = "pi_dash.bgtasks"

# Measured ~306s on the oracle (5 botocore attempts x 60s read timeout
# plus backoff). The bound leaves headroom for loaded CI without
# letting a hang pass.
STALL_BOUND_SECS = 480
# Any finish faster than one 60s timeout means the S3 call never stalled
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


def test_delete_old_s3_link_stall_fails_bounded(db_conn, broker_url):
    """A wedged S3 fails the expiry task within a bounded time (Django).

    The worker dials a black hole (accept, never reply). The S3 call
    must time out and fail the task instead of hanging; a sentinel
    ``delete_api_logs`` job published right after proves the worker is
    free again, and the expired row's ``url`` must still be set
    (fail-stop: Django clears it only after the delete succeeds).
    """
    owner = seed_helpers.user(db_conn, "contract-stall-owner")
    workspace = seed_helpers.workspace(db_conn, "contractstall", owner["id"])
    row = insert_row(
        db_conn,
        "exporters",
        {
            "workspace_id": str(workspace["id"]),
            "type": "issue_exports",
            "provider": "csv",
            "status": "completed",
            "reason": "",
            "url": "https://example.com/stall-expired.zip",
            "key": f"contract-stall-{uuid.uuid4().hex}.zip",
            "token": f"contract-{uuid.uuid4().hex}",
            "initiated_by_id": str(owner["id"]),
            "created_at": _old(days=9),
        },
    )
    sentinel = insert_row(
        db_conn,
        "api_activity_logs",
        {
            "token_identifier": "contract-stall-probe",
            "path": "/contract/stall-old",
            "method": "GET",
            "response_code": 200,
            "created_at": _old(),
        },
    )
    with stall.black_hole() as listener:
        started = time.monotonic()
        celery_wire.publish(f"{M}.exporter_expired_task.delete_old_s3_link")
        celery_wire.publish(f"{M}.cleanup_task.delete_api_logs")
        wait_for(
            lambda: _missing(db_conn, "api_activity_logs", sentinel["id"]),
            timeout=STALL_BOUND_SECS,
            what=f"sentinel api_activity_logs row deleted after stalled expiry (bound {STALL_BOUND_SECS}s)",
        )
        elapsed = time.monotonic() - started
    assert listener.hits >= 1, "worker never dialed the stall endpoint"
    assert elapsed >= MIN_STALL_SECS, f"finished in {elapsed:.1f}s without stalling?"
    assert not _column_is_null(
        db_conn, "exporters", row["id"], "url"
    ), "expired url cleared despite the S3 failure"
