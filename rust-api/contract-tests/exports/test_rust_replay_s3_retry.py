# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Retry oracle, Rust-side replay (PIDASHCONV-1727).

Executable form of ``test_s3_retry.py`` against the Rust worker: the
``rust_job_queue`` row carries the exact Celery v2 payload (see
``_harness/rust_queue.py``) and the assertions are the same DB
before/after checks plus the stub attempt counts.

Stub environment only: the Rust worker under test boots with
``AWS_S3_ENDPOINT_URL=http://127.0.0.1:<S3RETRY_S3_PORT>`` (default
4598), ``USE_MINIO=1``, and ``--concurrency 1``. This module runs
alone in its own CI job (see
``rust-api-contract-tests-exports-s3retry.yml``); the shared replay
job points at live S3, and the Django worker-plane job ignores
``test_rust_replay*.py`` by glob.

Expected (Django behavior, botocore legacy retry): a transient S3
failure is retried up to five attempts with ``rand * 2**(n-1)``
backoff. Before the fix the Rust upload sent one attempt, so a single
503 failed the exporter row immediately and the recovery test below
fails pre-fix (failed row, one PUT).

Local run (own scratch database, Django worker STOPPED)::

    export DATABASE_URL=postgresql://... S3RETRY_S3_PORT=4598
    export AWS_S3_ENDPOINT_URL=http://127.0.0.1:4598 USE_MINIO=1
    pidash-api worker --concurrency 1 &  # Rust worker; Django worker stopped
    pytest -p no:django exports/test_rust_replay_s3_retry.py
"""

import uuid

from _harness import rust_queue, s3stub
from _harness.db import wait_for_condition as wait_for
from _harness.seed import Seed

from . import cases

SENTINEL_TASK = "pi_dash.bgtasks.cleanup_task.delete_api_logs"

PUT_OK = ("respond", 200, {}, b"")


def _respond(status, code=None, body=None, headers=None):
    if body is None and code is not None:
        body = s3stub.s3_error_xml(code, f"stub {code}")
    return ("respond", status, headers or {}, body or b"")


def _object_attempts(stub, method):
    """Hits for object requests (path-style PUT on a key, not HeadBucket)."""
    return sum(
        1
        for hit in stub.hits
        if hit.method == method and len(hit.path.split("/")) > 2
    )


def _wait_exporter(token: str, db_conn, what: str) -> dict:
    def settled():
        row = cases.read_exporter(db_conn, token)
        return row if row["status"] in ("completed", "failed") else None

    return wait_for(settled, what=what)


def _publish_issue_export(*, world, token):
    return rust_queue.publish(
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


def test_rust_issue_export_recovers_after_503(db_conn):
    """One 503 then success: the Rust worker retries and the export completes."""
    seed = Seed(db_conn)
    try:
        world = cases.seed_export_world(seed, cases.unique_tag("retry"))
        token = cases.unique_tag("tok")
        cases.seed_exporter(
            seed,
            workspace_id=world["workspace"],
            provider="csv",
            token=token,
            initiated_by_id=world["owner"],
        )
        script = [_respond(503, "SlowDown"), PUT_OK]
        with s3stub.stub_s3(script) as stub:
            celery_id = _publish_issue_export(world=world, token=token)
            row = _wait_exporter(token, db_conn, "rust issue export after 503")
            rust_queue.wait_for_settled(celery_id, what="rust retry settled")
            assert row["status"] == "completed", row
            assert row["reason"] == ""
            assert row["key"] == cases.expected_s3_key(world, token), row
            assert row["url"], row
            assert _object_attempts(stub, "PUT") == 2
    finally:
        cases.cleanup_export_world(seed)
        seed.cleanup()
