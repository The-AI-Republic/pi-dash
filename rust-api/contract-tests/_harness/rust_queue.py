# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Rust-worker replay publisher (PIDASHCONV-225, D-09 fix).

The Django oracle (``celery_wire.publish``) publishes jobs in Celery
protocol v2 to ``CELERY_BROKER_URL`` and probes the worker with Celery
broadcast inspect (``broker_probe``). Only a Celery (Django) worker can
execute or answer those, so the suite as written can only go green
against Django.

The Rust worker (``rust-api/crates/jobs/src/worker.rs:run_worker``)
claims rows from the Postgres queue ``rust_job_queue``
(``status='queued' AND visible_at <= now() ORDER BY visible_at, id
LIMIT 1 FOR UPDATE SKIP LOCKED``) and routes by registered task name:
a locally-registered name runs its handler, anything else forwards to
the Python plane over AMQP. This module is the executable Rust-side
replay form: it seeds ``rust_job_queue`` rows carrying the exact Celery
v2 payloads (``task``, ``args`` JSON array, ``kwargs`` JSON object) and
waits on the same Postgres table, so the identical DB before/after
assertions run with the Django worker stopped.

Payload mapping (mirrors ``queue::NewJob`` / ``enqueue_sql``):

- ``task`` — the full Celery task name, unchanged.
- ``args`` / ``kwargs`` — the Celery v2 body parts, unchanged.
- ``countdown`` — delivery delay in seconds, mapped to
  ``visible_at = now() + countdown`` (mirrors
  ``NewJob::delayed``; the worker will not claim the row before then,
  which is the ETA-parity half of ``test_eta_delayed_execution``).
- Task-level ``countdown`` inside ``kwargs`` (e.g. the version
  backfills' ``{"batch_size": 1, "countdown": 300}``) is payload, never
  a delivery delay: it passes through untouched.

Nothing here imports Django or touches the broker. The Django worker
must be stopped while the replay runs; the Rust worker
(``pidash-api worker``) must be running against the same
``DATABASE_URL``.
"""

from __future__ import annotations

import uuid
from datetime import datetime, timedelta, timezone

from psycopg.types.json import Json

from . import config
from .db import connect, wait_for_condition


QUEUE_TABLE = "rust_job_queue"


def publish(
    task_name: str,
    args=None,
    kwargs=None,
    countdown: float | None = None,
    queue: str = "celery",
) -> str:
    """Enqueue one job for the Rust worker. Returns the celery id."""
    celery_id = str(uuid.uuid4())
    payload_args = list(args or [])
    payload_kwargs = dict(kwargs or {})
    if countdown is not None:
        visible_at = datetime.now(timezone.utc) + timedelta(seconds=countdown)
    else:
        visible_at = None
    with connect() as conn, conn.cursor() as cur:
        cur.execute(
            f"INSERT INTO {QUEUE_TABLE} "
            "(celery_id, task, args, kwargs, queue, visible_at) "
            "VALUES (%s, %s, %s, %s, %s, COALESCE(%s, now())) "
            "RETURNING id",
            (
                celery_id,
                task_name,
                Json(payload_args),
                Json(payload_kwargs),
                queue,
                visible_at,
            ),
        )
        cur.fetchone()
    return celery_id


def queue_depth(queue: str = "celery") -> int:
    """Pending-row count the Rust worker has yet to claim."""
    with connect() as conn, conn.cursor() as cur:
        cur.execute(
            f"SELECT count(*) AS n FROM {QUEUE_TABLE} "
            "WHERE queue = %s AND status = 'queued' AND visible_at <= now()",
            (queue,),
        )
        return cur.fetchone()[0]


def row_present(celery_id: str) -> bool:
    """True while the worker has not settled the row (ack deletes)."""
    with connect() as conn, conn.cursor() as cur:
        cur.execute(
            f"SELECT 1 FROM {QUEUE_TABLE} WHERE celery_id = %s",
            (celery_id,),
        )
        return cur.fetchone() is not None


def wait_for_settled(celery_id: str, what: str = "rust job settled") -> None:
    """Wait until the worker acks (deletes) the enqueued row."""
    wait_for_condition(lambda: not row_present(celery_id) or None, what=what)


def wait_for_queue_drain(baseline: int = 0, what: str = "rust queue drain") -> None:
    """Wait until the Rust worker has consumed every published job."""
    wait_for_condition(lambda: queue_depth() <= baseline or None, what=what)


def broker_url() -> str:
    """Broker URL for export-forwarding observation (AMQP half)."""
    return config.required(config.CELERY_BROKER_URL)
