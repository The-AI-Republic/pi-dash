# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-09 mongo oracle: ``delete_api_logs`` archives ``ip_address`` without CIDR (PIDASHCONV-1717).

Mongo environment only: the Django worker under test boots with
``MONGO_DB_URL`` / ``MONGO_DB_DATABASE`` pointing at a live MongoDB.
Runs alone in its own CI job (see
``rust-api-contract-tests-tasks-cleanup-mongo.yml``); the shared
worker-plane job ignores it — its worker boots without Mongo.

Django behavior (``cleanup_task.py:274-291`` + ``:180``): the ``inet``
column is loaded as plain text (``10.0.0.1``) and copied as-is, so the
archived document holds the host address. The Rust SeaORM rewrite
(``3a8c993a0``) used ``CAST("ip_address" AS text)`` which appends the
netmask (``10.0.0.1/32``, ``2001:db8::1/128``) — this test fails on that
archive and pins the ``abbrev()`` fix.

Expected: archived ``ip_address`` is ``"10.0.0.1"`` for an IPv4 host,
``"2001:db8::1"`` for an IPv6 host, and ``null`` for ``NULL``; the
Postgres rows are hard-deleted. Before the fix the Rust replay archives
``"10.0.0.1/32"`` / ``"2001:db8::1/128"`` and this test fails on the
value assertion.

Local run (own scratch database, Django worker only)::

    export DATABASE_URL=postgresql://...
    export MONGO_DB_URL=mongodb://127.0.0.1:27017 MONGO_DB_DATABASE=pidash_ct
    export CELERY_BROKER_URL=redis://... # same broker the worker uses
    celery -A pi_dash worker -E --pool=solo -I pi_dash.bgtasks &
    pytest -p no:django tasks_cleanup/test_api_logs_ip_abbrev.py
"""

import uuid
from datetime import datetime, timedelta, timezone

from pymongo import MongoClient

from _harness import celery_wire
from _harness import config as harness_config
from _harness.db import insert_row, wait_for_condition as wait_for

M = "pi_dash.bgtasks"
COLLECTION = "api_activity_logs"

SETTLE_BOUND_SECS = 300


def _mongo_collection():
    client = MongoClient(
        harness_config.required("MONGO_DB_URL"), serverSelectionTimeoutMS=10000
    )
    return client[harness_config.required("MONGO_DB_DATABASE")][COLLECTION]


def _old(days: int = 60) -> str:
    return (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()


def _missing(conn, table: str, row_id: str) -> bool:
    with conn.cursor() as cur:
        cur.execute(f"SELECT 1 FROM {table} WHERE id = %s", (row_id,))
        return cur.fetchone() is None


def _seed_three(db_conn):
    """Seed one IPv4, one IPv6, and one NULL aged row; return id map."""
    # Ensure a clean Mongo collection so assertions are deterministic.
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


def test_api_logs_ip_address_is_host_not_cidr(db_conn):
    """Archived ip_address is the host address, not the CIDR, and NULL stays null."""
    ids = _seed_three(db_conn)

    celery_wire.publish(f"{M}.cleanup_task.delete_api_logs")

    def _all_deleted():
        # Poll until every seeded row is gone from Postgres.
        return all(_missing(db_conn, COLLECTION, rid) for rid in ids.values())

    wait_for(_all_deleted, timeout=SETTLE_BOUND_SECS, what="aged api_activity_logs rows deleted")

    # Mongo side: the archive is the only copy left, so its ip_address must match Django.
    coll = _mongo_collection()
    docs = {d["id"]: d for d in coll.find({"id": {"$in": sorted(ids.values())}})}
    assert len(docs) == 3, f"expected 3 archived docs, got {len(docs)}: {docs}"

    by_ip = {}
    for label, rid in ids.items():
        by_ip[label] = docs[rid].get("ip_address")

    assert by_ip["v4"] == "10.0.0.1", f'IPv4 archived as {by_ip["v4"]!r}, expected "10.0.0.1"'
    assert by_ip["v6"] == "2001:db8::1", f'IPv6 archived as {by_ip["v6"]!r}, expected "2001:db8::1"'
    assert by_ip["null"] is None, f'NULL archived as {by_ip["null"]!r}, expected None'

    # No CIDR suffix may survive on either address family.
    for label, val in by_ip.items():
        if val is not None:
            assert "/" not in val, f"archived ip_address for {label} contains CIDR suffix: {val!r}"
