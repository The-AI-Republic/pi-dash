# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-09 mongo oracle, Rust-side replay (PIDASHCONV-1290).

Executable form of ``test_mongo_write_error.py`` against the Rust worker:
the ``rust_job_queue`` row carries the exact Celery v2 payload (see
``_harness/rust_queue.py``) and the assertions are the same DB
before/after checks plus the Rust-observable job state (acked, not
requeued).

Mongo environment only: the Rust worker under test boots with
``MONGO_DB_URL`` / ``MONGO_DB_DATABASE`` pointing at a live MongoDB.
This module runs alone in its own CI job (see
``rust-api-contract-tests-tasks-cleanup-mongo.yml``); the shared replay
job pins the live file only, and the Django worker-plane job ignores
``test_rust_replay*.py`` by glob.

Expected (Django behavior): the failing first batch is skipped (its 500
rows stay in Postgres), the second batch is archived and deleted, and
the job completes. Before the fix the ``insert_many`` write error is
misclassified as ``CleanupError::MongoTransport``: the run aborts at
batch 1, nothing is deleted, and the job is requeued — this test fails
on the requeue, quoting the transport error.

Local run (own scratch database, Django worker STOPPED)::

    export DATABASE_URL=postgresql://...
    export MONGO_DB_URL=mongodb://127.0.0.1:27017 MONGO_DB_DATABASE=pidash_ct
    pidash-api worker --concurrency 1 &  # Rust worker; Django worker stopped
    pytest -p no:django tasks_cleanup/test_rust_replay_mongo_write_error.py
"""

import uuid
from datetime import datetime, timedelta, timezone

from pymongo import MongoClient

from _harness import rust_queue
from _harness import config as harness_config
from _harness.db import wait_for_condition as wait_for

M = "pi_dash.bgtasks"

# Same two-batch shape as the oracle: 600 aged rows flush as 500 + 100,
# and the conflict row sorts first in the ``-created_at`` order.
N_ROWS = 600
BATCH_SIZE = 500
COLLECTION = "api_activity_logs"

# Task time is sub-second; the bound covers queue pickup on a loaded
# runner. A pre-fix run fails fast (first attempt requeues), so the full
# bound only bites on a real hang.
SETTLE_BOUND_SECS = 120


def _mongo_collection():
    client = MongoClient(
        harness_config.required("MONGO_DB_URL"), serverSelectionTimeoutMS=10000
    )
    return client[harness_config.required("MONGO_DB_DATABASE")][COLLECTION]


def _seed_two_batches(conn):
    """Wipe + seed exactly 600 aged rows; return (batch1, batch2, newest)."""
    with conn.cursor() as cur:
        cur.execute(f'DELETE FROM "{COLLECTION}"')
        now = datetime.now(timezone.utc)
        rows = []
        for i in range(1, N_ROWS + 1):
            ts = now - timedelta(minutes=(N_ROWS + 1 - i), seconds=30)
            rows.append(
                (
                    str(uuid.uuid4()),
                    f"mongo-write-error-{i}",
                    "/contract/mongo-old",
                    "GET",
                    200,
                    "10.9.9.9",
                    ts,
                    ts,
                )
            )
        cur.executemany(
            f'INSERT INTO "{COLLECTION}" '
            "(id, token_identifier, path, method, response_code,"
            " ip_address, created_at, updated_at) "
            "VALUES (%s::uuid, %s, %s, %s, %s, %s::inet, %s, %s)",
            rows,
        )
        cur.execute(f'SELECT id FROM "{COLLECTION}" ORDER BY created_at DESC')
        ordered = [str(r[0]) for r in cur.fetchall()]
    assert len(ordered) == N_ROWS
    return set(ordered[:BATCH_SIZE]), set(ordered[BATCH_SIZE:]), ordered[0]


def _plant_conflict(conflict_id: str) -> None:
    """Force batch 1's archive write to fail with E11000 on ``id``."""
    coll = _mongo_collection()
    coll.drop()
    coll.insert_one({"id": conflict_id, "seed": "pidashconv-1290-conflict"})
    coll.create_index("id", unique=True, name="pidashconv_1290_id_unique")


def _present(conn, ids: set) -> set:
    with conn.cursor() as cur:
        cur.execute(f'SELECT id FROM "{COLLECTION}"')
        remaining = {str(r[0]) for r in cur.fetchall()}
    return remaining & ids


def _job_terminal(conn, celery_id: str):
    """Acked (completed) or requeued/failed row, else None (still pending)."""
    with conn.cursor() as cur:
        cur.execute(
            "SELECT status, attempts, last_error FROM rust_job_queue WHERE celery_id = %s",
            (celery_id,),
        )
        row = cur.fetchone()
    if row is None:
        return ("acked",)
    status, attempts, last_error = row
    if status == "failed" or attempts >= 1:
        return (status, attempts, last_error)
    return None


def _delete_job(conn, celery_id: str) -> None:
    with conn.cursor() as cur:
        cur.execute("DELETE FROM rust_job_queue WHERE celery_id = %s", (celery_id,))


def test_rust_mongo_write_error_skips_batch(db_conn):
    """A failing first batch is skipped, not a transport abort + requeue."""
    batch1, batch2, conflict_id = _seed_two_batches(db_conn)
    assert conflict_id in batch1
    _plant_conflict(conflict_id)

    job_id = rust_queue.publish(f"{M}.cleanup_task.delete_api_logs")
    try:
        terminal = wait_for(
            lambda: _job_terminal(db_conn, job_id),
            timeout=SETTLE_BOUND_SECS,
            what=f"cleanup job settled (bound {SETTLE_BOUND_SECS}s)",
        )
        assert terminal[0] == "acked", (
            f"job did not complete: status={terminal[0]!r} "
            f"attempts={terminal[1]} last_error={terminal[2]!r}"
        )
        assert not _present(db_conn, batch2), "second batch was not deleted"
        assert _present(db_conn, batch1) == batch1, (
            "first batch was not skipped: "
            f"{len(batch1 - _present(db_conn, batch1))} of its 500 rows were deleted"
        )
        coll = _mongo_collection()
        assert coll.count_documents({"id": {"$in": sorted(batch2)}}) == len(batch2)
    finally:
        # A bug that requeues must not keep retrying into later tests.
        _delete_job(db_conn, job_id)
