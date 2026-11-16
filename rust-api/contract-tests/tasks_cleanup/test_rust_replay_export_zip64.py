# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Small-export zip container through the Rust worker (PIDASHCONV-1700).

Replay of ``test_export_zip64.py``: the same under-limit issue export
must come back as plain zip (``testzip()`` clean, no zip64
end-of-central-directory records, every entry at version 20).

Run with the Django worker STOPPED and the Rust worker running
(``pidash-api worker``) against the same ``DATABASE_URL``, as the
``exports/`` replay suite does.
"""

from __future__ import annotations

import io
import zipfile

import pytest

from _harness import rust_queue
from _harness.db import wait_for_condition as wait_for
from _harness.seed import Seed
from exports import cases


@pytest.fixture()
def seed(db_conn):
    helper = Seed(db_conn)
    yield helper
    cases.cleanup_export_world(helper)
    helper.cleanup()


def _wait_exporter(token: str, db_conn, what: str) -> dict:
    def settled():
        row = cases.read_exporter(db_conn, token)
        # Terminal only: "processing" is transient on both backends.
        return row if row["status"] in ("completed", "failed") else None

    return wait_for(settled, what=what)


def test_rust_issue_export_zip_is_plain(seed, db_conn):
    """Rust wraps a small export in plain zip (no zip64 records)."""
    world = cases.seed_export_world(seed, cases.unique_tag("z64"))
    token = cases.unique_tag("tok")
    cases.seed_exporter(
        seed,
        workspace_id=world["workspace"],
        provider="json",
        token=token,
        initiated_by_id=world["owner"],
    )
    celery_id = rust_queue.publish(
        cases.ISSUE_EXPORT_TASK,
        kwargs={
            "provider": "json",
            "workspace_id": world["workspace"],
            "project_ids": [world["project"]],
            "token_id": token,
            "multiple": False,
            "slug": world["slug"],
        },
    )
    row = _wait_exporter(token, db_conn, "rust plain zip export")
    assert row["status"] == "completed", row
    rust_queue.wait_for_settled(celery_id, what="rust plain zip settled")
    client = cases.s3_client()
    zipped = cases.get_object_bytes(client, row["key"])
    assert b"PK\x06\x06" not in zipped, "zip64 EOCD in an under-limit export"
    assert b"PK\x06\x07" not in zipped, "zip64 locator in an under-limit export"
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        assert archive.testzip() is None
        for info in archive.infolist():
            assert info.extract_version == 20, info.filename
