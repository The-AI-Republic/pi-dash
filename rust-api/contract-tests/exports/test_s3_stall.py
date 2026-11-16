# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stall oracle: ``issue_export_task`` against a wedged S3 (PIDASHCONV-1727).

Stall environment only: the Django worker under test boots with
``AWS_S3_ENDPOINT_URL=http://127.0.0.1:<STALL_S3_PORT>`` (default
4599), ``USE_MINIO=1``, and serial execution (``--pool=solo``). This
module runs alone in its own CI job (see
``rust-api-contract-tests-exports-stall.yml``); the shared
worker-plane job points at live S3, and a live endpoint would answer
the upload instead of stalling.

Django behavior (``export_task.py:42-124``): the boto3 client
carries no timeout ``Config``, so botocore's defaults apply (60s
connect / 60s read, plus legacy retries) -- the task fails after
about 5x60s and the exporter row becomes ``failed`` with a reason.
The Rust port must bound its client the same way (see
``test_rust_replay_s3_stall.py``); this module pins the Django side
of that contract and passes throughout.

Local run (own scratch database, Django worker only)::

    export DATABASE_URL=postgresql://... STALL_S3_PORT=4599
    export AWS_S3_ENDPOINT_URL=http://127.0.0.1:4599 USE_MINIO=1
    export CELERY_BROKER_URL=redis://... # same broker the worker uses
    celery -A pi_dash worker -E --pool=solo -I pi_dash.bgtasks.export_task &
    pytest -p no:django exports/test_s3_stall.py
"""

import time
from datetime import datetime, timedelta, timezone

import pytest

from _harness import celery_wire, stall
from _harness.db import insert_row, wait_for_condition as wait_for
from _harness.export_worker import spawn_export_worker
from _harness.seed import Seed

from . import cases

SENTINEL_TASK = "pi_dash.bgtasks.cleanup_task.delete_api_logs"

# Measured ~315s on the oracle (5 botocore attempts x 60s read timeout
# plus backoff). The bound leaves headroom for loaded CI without
# letting a hang pass.
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


@pytest.fixture(scope="session")
def export_worker():
    worker = spawn_export_worker()
    yield worker
    worker.stop()


@pytest.fixture()
def seed(db_conn):
    helper = Seed(db_conn)
    yield helper
    cases.cleanup_export_world(helper)
    helper.cleanup()


def _wait_exporter(token: str, db_conn, what: str) -> dict:
    def settled():
        row = cases.read_exporter(db_conn, token)
        return row if row["status"] in ("completed", "failed") else None

    return wait_for(settled, what=what, timeout=STALL_BOUND_SECS)


def test_issue_export_stall_fails_bounded(export_worker, seed, db_conn):
    """A wedged S3 fails the issue export within a bounded time (Django).

    The worker dials a black hole (accept, never reply). The upload
    must time out and exhaust the retries, ending with the exporter
    row ``failed`` instead of staying ``processing`` forever; a
    sentinel ``delete_api_logs`` job published right after proves the
    worker is free again.
    """
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
            "token_identifier": "export-stall-probe",
            "path": "/contract/export-stall-old",
            "method": "GET",
            "response_code": 200,
            "created_at": _old(),
        },
    )
    with stall.black_hole() as listener:
        started = time.monotonic()
        celery_wire.publish(
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
        celery_wire.publish(SENTINEL_TASK)
        row = _wait_exporter(token, db_conn, "stalled issue export failed")
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
