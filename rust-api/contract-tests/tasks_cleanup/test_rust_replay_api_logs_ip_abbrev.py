# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-09 mongo oracle, Rust-side replay (PIDASHCONV-1717).

Executable form of ``test_api_logs_ip_abbrev.py`` against the Rust worker:
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

Expected (Django behavior): the archived ``ip_address`` is
``"10.0.0.1"`` for an IPv4 host, ``"2001:db8::1"`` for an IPv6 host,
and ``null`` for ``NULL``; the Postgres rows are hard-deleted. Before
the fix the SeaORM path archives ``"10.0.0.1/32"`` / ``"2001:db8::1/128"``
via ``CAST AS text`` and this test fails on the value assertion; after
the fix it routes through ``abbrev()`` and passes.

Local run (own scratch database, Django worker STOPPED)::

    export DATABASE_URL=postgresql://...
    export MONGO_DB_URL=mongodb://127.0.0.1:27017 MONGO_DB_DATABASE=pidash_ct
    pidash-api worker --concurrency 1 &  # Rust worker; Django worker stopped
    pytest -p no:django tasks_cleanup/test_rust_replay_api_logs_ip_abbrev.py
"""

import uuid
from datetime import datetime, timedelta, timezone

from pymongo import MongoClient

from _harness import rust_queue
from _harness import config as harness_config
from _harness.db import insert_row, wait_for_condition as wait_for

M = "pi_dash.bgtasks"
COLLECTION = "api_activity_logs"

SETTLE_BOUND_SECS = 120


def _mongo_collection():
    client = MongoClient(
        harness_config.required("MONGO_DB_URL"), serverSelectionTimeoutMS=10000
    )
    return client[harness_config.required("MONGO_DB_DATABASE")][COLLECTION]


def _seed_three(db_conn):
    _mongo_collection().drop()
    base = f"ip-abbrev-{uuid.uuid4().hex[:8]}"
    aged = (datetime.now(timezone.utc) - timedelta(days=60)).isoformat()
    ids = {}
    for label, ip in [("v4", "10.0.0.1"), ("v6", "2001:db8::1"), ("null", None)]:
        row = insert_row(
            db_conn,
            COLLECTION,
            {
                "token_identifier": f"{base}-{label}",
                "path": "/contract/ip-abbrev",
                "method": "GET",
                "response_code": 200,
                "ip_address": ip,
                "created_at": aged,
                "updated_at": aged,
            },
        )
        ids[label] = str(row["id"])
    return ids


def _missing(conn, row_id: str) -> bool:
    with conn.cursor() as cur:
        cur.execute(f"SELECT 1 FROM {COLLECTION} WHERE id = %s", (row_id,))
        return cur.fetchone() is None


def test_rust_api_logs_ip_address_is_host_not_cidr(db_conn):
    """Rust worker archives the host address, not the CIDR, and NULL stays null."""
    ids = _seed_three(db_conn)

    rust_queue.publish(f"{M}.cleanup_task.delete_api_logs")

    def _all_deleted():
        return all(_missing(db_conn, rid) for rid in ids.values())

    wait_for(_all_deleted, timeout=SETTLE_BOUND_SECS, what="aged api_activity_logs rows deleted by rust worker")

    coll = _mongo_collection()
    docs = {d["id"]: d for d in coll.find({"id": {"$in": sorted(ids.values())}})}
    assert len(docs) == 3, f"expected 3 archived docs, got {len(docs)}: {docs}"

    by_ip = {label: docs[rid].get("ip_address") for label, rid in ids.items()}
    assert by_ip["v4"] == "10.0.0.1", f'IPv4 archived as {by_ip["v4"]!r}, expected "10.0.0.1"'
    assert by_ip["v6"] == "2001:db8::1", f'IPv6 archived as {by_ip["v6"]!r}, expected "2001:db8::1"'
    assert by_ip["null"] is None, f'NULL archived as {by_ip["null"]!r}, expected None'
    for label, val in by_ip.items():
        if val is not None:
            assert "/" not in val, f"archived ip_address for {label} contains CIDR suffix: {val!r}"
