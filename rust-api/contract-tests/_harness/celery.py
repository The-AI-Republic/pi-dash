"""Celery-protocol (v2) task publisher for task-oracle suites.

Task-only domains (no HTTP routes) drive the worker the same way Django's
``task.delay()`` does: a protocol-2 JSON message LPUSHed onto the broker
queue named by ``CELERY_BROKER_URL`` (kombu redis transport LPUSHes;
consumers BRPOP). The envelope shape below is pinned against a message
captured from a real ``.delay()`` call (see ``dispatch/test_celery_wire.py``):
``body`` is base64 ``[args, kwargs, embed]``, ``headers`` carries the task
name / id / retries / eta / timelimit, ``properties`` carries the routing.

Only the ``redis`` broker scheme is supported: every local and contract
stack configures ``AMQP_URL=redis://...`` (see ``settings/common.py`` —
``CELERY_BROKER_URL`` falls back to RabbitMQ only when ``AMQP_URL`` is
unset). Anything else fails fast so a suite never silently publishes into
a broker no worker reads.
"""

from __future__ import annotations

import base64
import json
import os
import socket
import uuid
from urllib.parse import urlparse


def get_broker_url() -> str:
    """Broker URL for task-oracle suites: ``CELERY_BROKER_URL``, else ``AMQP_URL``."""
    url = os.environ.get("CELERY_BROKER_URL") or os.environ.get("AMQP_URL", "")
    if not url:
        raise RuntimeError(
            "CELERY_BROKER_URL (or AMQP_URL) is required: point it at the Redis "
            "broker of the server BASE_URL serves, e.g. "
            "CELERY_BROKER_URL=redis://localhost:6379/0"
        )
    scheme = urlparse(url).scheme
    if scheme != "redis":
        raise RuntimeError(
            f"unsupported broker scheme {scheme!r}: task-oracle suites only speak "
            "the redis broker protocol (LPUSH of Celery v2 JSON)."
        )
    return url


def _message_id() -> str:
    return str(uuid.uuid4())


class Broker:
    """Minimal Celery v2 publisher + queue inspector over Redis."""

    def __init__(self, url: str):
        import redis

        self._client = redis.Redis.from_url(url, decode_responses=True)
        # Fail fast when the broker is unreachable so suites report the
        # environment problem instead of timing out on a DB effect.
        self._client.ping()

    def publish(
        self,
        task: str,
        args: list | None = None,
        kwargs: dict | None = None,
        *,
        queue: str = "celery",
        timelimit: tuple | list | None = None,
    ) -> str:
        """Publish one task message exactly as ``task.delay(*args)`` would.

        ``timelimit`` is the ``[hard, soft]`` pair from the wire (``[None,
        None]`` when the task configures no limits, as for the sweep tasks).
        Returns the Celery task id.
        """
        args = list(args or [])
        kwargs = dict(kwargs or {})
        task_id = _message_id()
        body = base64.b64encode(
            json.dumps(
                [args, kwargs, {"callbacks": None, "errbacks": None, "chain": None, "chord": None}],
                separators=(",", ":"),
            ).encode()
        ).decode()
        message = {
            "body": body,
            "content-encoding": "utf-8",
            "content-type": "application/json",
            "headers": {
                "argsrepr": repr(args),
                "eta": None,
                "expires": None,
                "group": None,
                "group_index": None,
                "id": task_id,
                "ignore_result": False,
                "kwargsrepr": repr(kwargs),
                "lang": "py",
                "origin": f"gen{os.getpid()}@{socket.gethostname()}",
                "parent_id": None,
                "replaced_task_nesting": 0,
                "retries": 0,
                "root_id": task_id,
                "shadow": None,
                "stamped_headers": None,
                "stamps": {},
                "task": task,
                "timelimit": list(timelimit) if timelimit is not None else [None, None],
            },
            "properties": {
                "body_encoding": "base64",
                "correlation_id": task_id,
                "delivery_info": {"exchange": "", "routing_key": queue},
                "delivery_mode": 2,
                "delivery_tag": _message_id(),
                "priority": 0,
                "reply_to": _message_id(),
            },
        }
        self._client.lpush(queue, json.dumps(message))
        return task_id

    def queue_length(self, queue: str = "celery") -> int:
        return self._client.llen(queue)

    def read_last(self, queue: str, *, remove: bool = True):
        """Fetch one message off the head of ``queue`` (test side-queues only)."""
        raw = self._client.lpop(queue) if remove else None
        if raw is None:
            return None
        return json.loads(raw)

    def close(self):
        self._client.close()
