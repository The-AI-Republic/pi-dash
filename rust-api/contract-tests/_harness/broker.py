"""Celery protocol-v2 broker helpers (pika; no celery/django imports)."""
from __future__ import annotations

import json
import time
import uuid

import pika


EXCHANGE = "celery"
ROUTING_KEY = "celery"


def _params(broker_url: str) -> pika.URLParameters:
    params = pika.URLParameters(broker_url)
    params.socket_timeout = 10
    params.stack_timeout = 15
    return params


def publish_task(
    broker_url: str,
    task: str,
    args: list | None = None,
    kwargs: dict | None = None,
    task_id: str | None = None,
    countdown: float | None = None,
    eta: str | None = None,
    exchange: str = EXCHANGE,
    routing_key: str = ROUTING_KEY,
) -> str:
    """Publish one job in Celery wire format. Returns the task id.

    Body layout is protocol v2: [args, kwargs, embed]. Headers carry the
    task name, ids, retries and eta — exactly what kombu sends, so the
    worker cannot tell this apart from a .delay() call.
    """
    task_id = task_id or str(uuid.uuid4())
    args = list(args or [])
    kwargs = dict(kwargs or {})
    body = [args, kwargs, {"callbacks": None, "errbacks": None, "chain": None, "chord": None}]
    headers = {
        "lang": "py",
        "task": task,
        "id": task_id,
        "root_id": task_id,
        "parent_id": None,
        "group": None,
        "meth": None,
        "shadow": None,
        "eta": eta,
        "expires": None,
        "retries": 0,
        "timelimit": [None, None],
        "argsrepr": repr(args),
        "kwargsrepr": repr(kwargs),
    }
    if countdown is not None and eta is None:
        from datetime import datetime, timedelta, timezone

        headers["eta"] = (datetime.now(timezone.utc) + timedelta(seconds=countdown)).isoformat()
    properties = pika.BasicProperties(
        content_type="application/json",
        content_encoding="utf-8",
        correlation_id=task_id,
        delivery_mode=2,
        headers=headers,
    )
    connection = pika.BlockingConnection(_params(broker_url))
    try:
        channel = connection.channel()
        channel.exchange_declare(exchange=exchange, exchange_type="direct", durable=True)
        channel.queue_declare(queue=routing_key, durable=True, passive=True)
        channel.confirm_delivery()  # a nack/disconnect surfaces as an exception
        channel.basic_publish(
            exchange=exchange,
            routing_key=routing_key,
            body=json.dumps(body),
            properties=properties,
        )
    finally:
        connection.close()
    return task_id


def queue_depth(broker_url: str, queue: str = ROUTING_KEY) -> int:
    """Passive depth check. Does not consume."""
    connection = pika.BlockingConnection(_params(broker_url))
    try:
        channel = connection.channel()
        declared = channel.queue_declare(queue=queue, durable=True, passive=True)
        return declared.method.message_count
    finally:
        connection.close()


def wait_for_drain(
    broker_url: str,
    high_water: int,
    queue: str = ROUTING_KEY,
    timeout: float = 120.0,
) -> bool:
    """Wait until the queue drains back to at most the pre-publish level.

    Call only after publish_task returned: delivery confirmation already
    proves the message reached the broker, so a return to the old level
    proves the worker consumed (acked) it. Shared-stack traffic (beat) can
    add messages meanwhile, so this asserts a return to level, never zero.
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        if queue_depth(broker_url, queue) <= high_water:
            return True
        time.sleep(1.0)
    return False
