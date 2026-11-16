# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Small-export zip container through the Django oracle (PIDASHCONV-1700).

An under-limit issue export must be plain zip: ``testzip()`` clean, no
zip64 end-of-central-directory records, every entry at version 20. The
Rust replay (``test_rust_replay_export_zip64.py``) asserts the same
container shape through the Rust worker.

A past-the-limit export (a 4 GiB fixture) is too heavy for this
harness, so the writer's zip64 thresholds are pinned by
``pidash-services`` unit tests instead (``tasks_cleanup::exports``):
``zip_count_past_u16_uses_zip64_eocd`` fails before the fix and passes
after it, and scratch probes verified a real 4 GiB archive with
``zipfile`` on both sides (see the PR).
"""

from __future__ import annotations

import io
import zipfile

import pytest

from _harness import celery_wire
from _harness.db import wait_for_condition as wait_for
from _harness.export_worker import spawn_export_worker
from _harness.seed import Seed
from exports import cases


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
        # Terminal only: "processing" is transient on both backends.
        return row if row["status"] in ("completed", "failed") else None

    return wait_for(settled, what=what)


def test_oracle_issue_export_zip_is_plain(export_worker, seed, db_conn):
    """Django wraps a small export in plain zip (no zip64 records)."""
    world = cases.seed_export_world(seed, cases.unique_tag("z64"))
    token = cases.unique_tag("tok")
    cases.seed_exporter(
        seed,
        workspace_id=world["workspace"],
        provider="json",
        token=token,
        initiated_by_id=world["owner"],
    )
    celery_wire.publish(
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
    row = _wait_exporter(token, db_conn, "plain zip export")
    assert row["status"] == "completed", row
    client = cases.s3_client()
    zipped = cases.get_object_bytes(client, row["key"])
    assert b"PK\x06\x06" not in zipped, "zip64 EOCD in an under-limit export"
    assert b"PK\x06\x07" not in zipped, "zip64 locator in an under-limit export"
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        assert archive.testzip() is None
        for info in archive.infolist():
            assert info.extract_version == 20, info.filename
