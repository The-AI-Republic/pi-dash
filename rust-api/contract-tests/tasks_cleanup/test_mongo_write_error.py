# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-09 mongo oracle: ``delete_api_logs`` skips a batch whose archive write fails (PIDASHCONV-1290).

Mongo environment only: the Django worker under test boots with
``MONGO_DB_URL`` / ``MONGO_DB_DATABASE`` pointing at a live MongoDB, and
this module plants a conflicting document plus a unique index on ``id``
so the first batch's ``bulk_write`` raises ``BulkWriteError`` (E11000).
Runs alone in its own CI job (see
``rust-api-contract-tests-tasks-cleanup-mongo.yml``); the shared
worker-plane job ignores it — its worker boots without Mongo, and
without an archive write error there is no skip to assert.

Django behavior (``cleanup_task.py:73-84``): the failing batch is logged
as a MongoDB bulk write error and kept in Postgres; every later batch is
archived and deleted, and the task completes. The Rust replay
(``test_rust_replay_mongo_write_error.py``) pins the same contract and
fails until ``MongoSink::archive`` classifies
``ErrorKind::InsertMany`` as a batch skip instead of a transport abort.

Local run (own scratch database, Django worker only)::

    export DATABASE_URL=postgresql://...
    export MONGO_DB_URL=mongodb://127.0.0.1:27017 MONGO_DB_DATABASE=pidash_ct
    export CELERY_BROKER_URL=redis://... # same broker the worker uses
    celery -A pi_dash worker -E --pool=solo -I <oracle includes> &
    pytest -p no:django tasks_cleanup/test_mongo_write_error.py
"""

import uuid
from datetime import datetime, timedelta, timezone

from pymongo import MongoClient

from _harness import celery_wire
from _harness import config as harness_config
from _harness.db import wait_for_condition as wait_for

M = "pi_dash.bgtasks"

# Two batches: the queryset flushes every BATCH_SIZE rows
# (``cleanup_task.py:35``; ``BATCH_SIZE`` in ``cleanup_queries.rs``), so
# 600 aged rows flush as 500 + 100. The conflict row is the newest, i.e.
# first in the ``-created_at`` iteration order, so it fails batch 1.
N_ROWS = 600
BATCH_SIZE = 500
COLLECTION = "api_activity_logs"

# Task time is sub-second; the bound covers broker pickup latency on a
# loaded runner (single-digit minutes observed locally on an idle box).
SETTLE_BOUND_SECS = 300


def _mongo_collection():
    client = MongoClient(
        harness_config.required("MONGO_DB_URL"), serverSelectionTimeoutMS=10000
    )
    return client[harness_config.required("MONGO_DB_DATABASE")][COLLECTION]


def _seed_two_batches(conn):
    """Wipe + seed exactly 600 aged rows; return (batch1_ids, batch2_ids).

    The table must hold exactly these rows: batch membership is positional
    in the ``-created_at`` iteration order, so any leftover row would shift
    the boundary. Returns the newest 500 ids (batch 1, iterated first),
    the oldest 100 (batch 2), and the newest id (batch 1's first doc).
    """
    with conn.cursor() as cur:
        cur.execute(f'DELETE FROM "{COLLECTION}"')
        now = datetime.now(timezone.utc)
        rows = []
        for i in range(1, N_ROWS + 1):
            # Row i ages (601 - i) minutes: row 600 is the newest.
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


def test_mongo_write_error_skips_batch(db_conn):
    """A failing first batch stays in Postgres; later batches still go."""
    batch1, batch2, conflict_id = _seed_two_batches(db_conn)
    assert conflict_id in batch1
    _plant_conflict(conflict_id)

    celery_wire.publish(f"{M}.cleanup_task.delete_api_logs")
    wait_for(
        lambda: not _present(db_conn, batch2) or None,
        timeout=SETTLE_BOUND_SECS,
        what=f"second batch (100 rows) deleted after first-batch write error (bound {SETTLE_BOUND_SECS}s)",
    )
    assert _present(db_conn, batch1) == batch1, (
        "first batch was not skipped: "
        f"{len(batch1 - _present(db_conn, batch1))} of its 500 rows were deleted"
    )
    # The run really archived through Mongo (not a PG-only delete with the
    # archive silently skipped): batch 2's docs landed in the collection.
    coll = _mongo_collection()
    assert coll.count_documents({"id": {"$in": sorted(batch2)}}) == len(batch2)
