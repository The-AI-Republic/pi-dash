"""Live worker probes over CELERY_BROKER_URL (no Django imports).

``registered_tasks`` uses Celery's own broadcast inspect, so the oracle
verifies against the real worker instead of trusting static source reads.
"""

from celery import Celery

from . import config
from .celery_wire import make_app
from .db import wait_for_condition


def registered_tasks(app: Celery | None = None, timeout: float = 10.0) -> set[str]:
    inspect = (app or make_app(config.required(config.CELERY_BROKER_URL))).control.inspect(
        timeout=timeout
    )
    responses = inspect.registered() or {}
    names: set[str] = set()
    for _worker, tasks in responses.items():
        names.update(tasks)
    return names


def wait_for_registration(task_names: set[str], what: str = "worker registration") -> set[str]:
    found = wait_for_condition(
        lambda: registered_tasks() or None,
        what=f"{what} (any worker answering inspect)",
    )
    missing = set(task_names) - found
    assert not missing, (
        f"worker does not register tasks: {sorted(missing)} "
        f"(registered sample: {sorted(found)[:10]}, total {len(found)})"
    )
    return found


def queue_depth(queue: str = "celery") -> int:
    """Return the current pending-message count of a broker queue."""
    from kombu import Connection

    with Connection(config.required(config.CELERY_BROKER_URL)) as conn:
        channel = conn.default_channel
        queue_obj, message_count, _consumers = channel.queue_declare(
            queue, passive=True
        )
        return message_count


def drain_queue(queue: str = "celery", limit: int = 100) -> list:
    """Consume up to ``limit`` pending messages from a broker queue (kombu).

    Used to observe fan-out (scan tasks enqueueing fire_* jobs) and to leave
    a clean queue behind. Returns the decoded (headers, body) pairs.

    Only run against a dedicated contract worker/queue: draining acks every
    message it sees.
    """
    from kombu import Connection

    seen = []
    with Connection(config.required(config.CELERY_BROKER_URL)) as conn:
        queue_obj = conn.SimpleQueue(queue)
        try:
            for _ in range(limit):
                try:
                    message = queue_obj.get(block=False)
                except Exception:
                    break
                seen.append((message.headers, message.payload))
                message.ack()
        finally:
            queue_obj.close()
    return seen


def wait_for_queue_drain(baseline: int = 0, what: str = "queue drain") -> None:
    """Wait until the worker has consumed every published job (no drop)."""
    from .db import wait_for_condition

    wait_for_condition(lambda: queue_depth() <= baseline or None, what=what)


def collect_matching(predicate, queue: str = "celery", timeout: float | None = None) -> list:
    """Collect queued messages matching ``predicate(headers, payload)``.

    Non-matching messages are requeued untouched, so this never eats another
    test's jobs (unlike :func:`drain_queue`). Only run against a dedicated
    contract worker/queue. Returns the matched ``(headers, payload)`` pairs.
    """
    import time

    from kombu import Connection

    from . import config as _config

    def _poll():
        matched = []
        with Connection(_config.required(_config.CELERY_BROKER_URL)) as conn:
            queue_obj = conn.SimpleQueue(queue)
            try:
                while True:
                    try:
                        message = queue_obj.get(block=False)
                    except Exception:
                        break
                    try:
                        if predicate(message.headers, message.payload):
                            matched.append((message.headers, message.payload))
                            message.ack()
                        else:
                            message.requeue()
                    except Exception:
                        try:
                            message.requeue()
                        except Exception:
                            pass
                        raise
            finally:
                queue_obj.close()
        return matched or None

    deadline = time.monotonic() + (
        timeout if timeout is not None else _config.TASK_TIMEOUT_SECONDS
    )
    while True:
        found = _poll()
        if found:
            # One more sweep: a fan-out may publish siblings right behind.
            time.sleep(_config.POLL_INTERVAL_SECONDS)
            found.extend(m for m in (_poll() or []) if m not in found)
            return found
        if time.monotonic() >= deadline:
            raise TimeoutError("timed out waiting for matching broker message")
        time.sleep(_config.POLL_INTERVAL_SECONDS)
