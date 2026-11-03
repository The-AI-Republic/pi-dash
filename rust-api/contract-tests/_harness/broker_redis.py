"""Celery wire-format publisher and queue inspection (kombu, no Celery import).

Publishes exactly what a Celery client puts on the wire (protocol v2, JSON),
so the backend's worker cannot tell the suite apart from beat or another
worker. Only the redis transport is supported for queue inspection; publish
works with any kombu transport.
"""

from __future__ import annotations

import datetime as dt
import uuid

from kombu import Connection, Exchange, Queue

from . import env

CELERY_QUEUE = "celery"
CELERY_EXCHANGE = "celery"


def _connection() -> Connection:
    return Connection(env.CELERY_BROKER_URL)


def _queue(name: str = CELERY_QUEUE) -> Queue:
    exchange = Exchange(CELERY_EXCHANGE, "direct", durable=True)
    return Queue(name, exchange=exchange, routing_key=name, durable=True)


def publish_task(
    task: str,
    args: tuple = (),
    kwargs: dict | None = None,
    *,
    countdown: float | None = None,
    eta: dt.datetime | None = None,
    task_id: str | None = None,
    retries: int = 0,
    queue: str = CELERY_QUEUE,
) -> str:
    """Publish one Celery v2 message. Returns the task id.

    `countdown`/`eta` set the v2 `eta` header; the worker must not execute
    before it. Re-publishing with the same `task_id` models redelivery.
    """
    tid = task_id or uuid.uuid4().hex
    eta_iso = _eta_iso(countdown=countdown, eta=eta)
    body = [list(args), dict(kwargs or {}), {"callbacks": None, "errbacks": None, "chain": None, "chord": None}]
    headers = {
        "id": tid,
        "task": task,
        "lang": "py",
        "root_id": tid,
        "parent_id": None,
        "group": None,
        "meth": None,
        "shadow": None,
        "eta": eta_iso,
        "expires": None,
        "retries": retries,
        "timelimit": [None, None],
        "argsrepr": repr(tuple(args)),
        "kwargsrepr": repr(dict(kwargs or {})),
        "origin": "contract-tests",
    }
    with _connection() as conn:
        producer = conn.Producer()
        producer.publish(
            body,
            exchange=_queue(queue).exchange,
            routing_key=queue,
            declare=[_queue(queue)],
            headers=headers,
            properties={
                "correlation_id": tid,
                "delivery_mode": 2,
                "delivery_info": {"exchange": CELERY_EXCHANGE, "routing_key": queue},
            },
            serializer="json",
            retry=True,
        )
    return tid


def _eta_iso(countdown: float | None, eta: dt.datetime | None) -> str | None:
    if countdown is not None:
        moment = dt.datetime.now(dt.timezone.utc) + dt.timedelta(seconds=countdown)
    elif eta is not None:
        moment = eta if eta.tzinfo else eta.replace(tzinfo=dt.timezone.utc)
    else:
        return None
    return moment.isoformat()


def _qsize_or_zero(simple) -> int:
    try:
        return simple.qsize()
    except Exception:
        # Never-declared queue (fresh broker DB): no entries by definition.
        return 0


def queue_depth(queue: str = CELERY_QUEUE) -> int:
    """Visible (deliverable-now) entries on `queue`. Redis transport only."""
    with _connection() as conn:
        simple = conn.SimpleQueue(_queue(queue))
        try:
            return _qsize_or_zero(simple)
        finally:
            simple.close()


def purge(queue: str = CELERY_QUEUE) -> int:
    """Drop every pending entry on `queue`. Returns the dropped count."""
    with _connection() as conn:
        simple = conn.SimpleQueue(_queue(queue))
        try:
            if _qsize_or_zero(simple) == 0:
                return 0
            n = 0
            while True:
                try:
                    simple.get_nowait()
                except Exception:
                    break
                n += 1
            return n
        finally:
            simple.close()
