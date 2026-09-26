"""Celery wire-format publisher and structural assertions.

Jobs are published in Celery protocol v2 (JSON) through the broker named by
CELERY_BROKER_URL, using the reference kombu/celery implementation — the same
bytes the Rust port must reproduce. Nothing here imports Django.

Two modes share one code path:
- live: publish to CELERY_BROKER_URL and let the Django worker execute.
- memory: build the message in-process and return its parts, so the exact
  wire structure (task name, id, args, kwargs, eta, retries) can be asserted
  with no infrastructure.

Protocol v2 layout (celery 5.x): the message is
``(headers, properties, (args, kwargs, embed), sent_event)`` where headers
carry ``task`` / ``id`` / ``eta`` / ``retries`` / ``timelimit`` / routing and
the body carries ``(args, kwargs, embed)``.
"""

import uuid

from celery import Celery

from . import config

# Header keys every protocol-v2 message must carry.
PROTOCOL_V2_REQUIRED_HEADER_KEYS = {
    "lang",
    "task",
    "id",
    "eta",
    "expires",
    "retries",
    "timelimit",
    "root_id",
    "parent_id",
}


def make_app(broker_url: str) -> Celery:
    app = Celery("contract-oracle", broker=broker_url)
    app.conf.update(
        task_serializer="json",
        result_serializer="json",
        accept_content=["application/json"],
        task_protocol=2,
    )
    return app


def publish(
    task_name: str,
    args=None,
    kwargs=None,
    countdown: float | None = None,
    eta=None,
    broker_url: str | None = None,
) -> str:
    """Publish one job in Celery wire format. Returns the task id."""
    app = make_app(broker_url or config.required(config.CELERY_BROKER_URL))
    result = app.send_task(
        task_name,
        args=list(args or []),
        kwargs=dict(kwargs or {}),
        countdown=countdown,
        eta=eta,
    )
    return result.id


def capture_wire_message(
    task_name: str,
    args=None,
    kwargs=None,
    countdown: float | None = None,
) -> tuple[dict, dict, tuple]:
    """Build a protocol-v2 message in-process; return (headers, props, body)."""
    app = make_app("memory://")
    headers, properties, body, _sent_event = app.amqp.create_task_message(
        str(uuid.uuid4()),
        task_name,
        args=list(args or []),
        kwargs=dict(kwargs or {}),
        countdown=countdown,
    )
    return headers, properties, body


def assert_wire_message(
    message: tuple[dict, dict, tuple],
    task_name: str,
    args=None,
    kwargs=None,
) -> None:
    """Payload parity: the wire message carries exactly this task + payload."""
    headers, _properties, body = message
    missing = PROTOCOL_V2_REQUIRED_HEADER_KEYS - set(headers)
    assert not missing, f"wire headers missing protocol v2 keys: {sorted(missing)}"
    assert headers["task"] == task_name
    assert headers["lang"] == "py"
    wire_args, wire_kwargs, _embed = body
    assert list(wire_args) == list(args or [])
    assert dict(wire_kwargs) == dict(kwargs or {})
