# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Retry oracle: issue-export S3 upload against a scripted stub (PIDASHCONV-1727).

Stub environment only: the Django worker under test boots with
``AWS_S3_ENDPOINT_URL=http://127.0.0.1:<S3RETRY_S3_PORT>`` (default
4598), ``USE_MINIO=1``, and ``--pool=solo``. This module runs alone
in its own CI job (see
``rust-api-contract-tests-exports-s3retry.yml``); the shared
worker-plane job points at live S3, and would answer the upload
instead of the script.

Django behavior (``export_task.py:50-62``): the boto3 client sets
only the signature version, so botocore's legacy retry mode applies
-- 5 attempts in total with ``rand * 2**(n-1)`` backoff. A single
503 is retried and the upload lands. This module pins the Django
side of that contract and passes throughout.

Local run (own scratch database, Django worker only)::

    export DATABASE_URL=postgresql://... S3RETRY_S3_PORT=4598
    export AWS_S3_ENDPOINT_URL=http://127.0.0.1:4598 USE_MINIO=1
    export CELERY_BROKER_URL=redis://... # same broker the worker uses
    celery -A pi_dash worker -E --pool=solo -I pi_dash.bgtasks.export_task &
    pytest -p no:django exports/test_s3_retry.py
"""

import pytest

from _harness import celery_wire, s3stub
from _harness.db import wait_for_condition as wait_for
from _harness.export_worker import spawn_export_worker
from _harness.seed import Seed

from . import cases

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

    return wait_for(settled, what=what)


def _publish_issue_export(*, world, token):
    return celery_wire.publish(
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


def test_issue_export_recovers_after_503(export_worker, seed, db_conn):
    """One 503 then success: Django retries and the export completes."""
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
        _publish_issue_export(world=world, token=token)
        row = _wait_exporter(token, db_conn, "issue export after 503")
        assert row["status"] == "completed", row
        assert row["reason"] == ""
        assert row["key"] == cases.expected_s3_key(world, token), row
        assert row["url"], row
        assert _object_attempts(stub, "PUT") == 2
