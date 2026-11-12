"""Celery wire-format parity (PIDASHCONV-22, D-11).

Pins the exact protocol-2 envelope Django's ``task.delay()`` sends for each
dispatch task, captured from a live ``.delay()`` call:

- ``cloud_agent.run_agent_run``: ``args == [str(run_id)]``, ``kwargs == {}``,
  ``timelimit == [hard, soft]`` (from ``CLOUD_AGENT_RUN_HARD_LIMIT_SECONDS``
  / ``..._SOFT_LIMIT_SECONDS`` — asserted as ordered positive ints, not
  pinned values, since they are deployment settings).
- ``cloud_agent.scan_queued_runs`` / ``cloud_agent.sweep_stale_runs`` /
  ``managed_runner.expire_waiting_runs``: no args, ``timelimit ==
  [None, None]`` (no limits configured).
- All: ``eta``/``expires`` null (``delay`` semantics — no scheduled
  delivery), ``retries == 0``, ``content-type == application/json``,
  ``delivery_info`` routed to the ``celery`` queue, ``body`` base64
  ``[args, kwargs, {callbacks/errbacks/chain/chord: null}]``.

Messages here go to a throwaway side queue no worker consumes, so the
assertions read back byte-identical envelopes. The execution suites publish
the same envelopes to the real queue.
"""

import base64
import json
import uuid

import pytest

from .conftest import CLOUD_AGENT, MANAGED_RUNNER

pytestmark = pytest.mark.contract

RUN_TASK = "cloud_agent.run_agent_run"
SCAN_TASK = "cloud_agent.scan_queued_runs"
SWEEP_TASK = "cloud_agent.sweep_stale_runs"
EXPIRE_TASK = "managed_runner.expire_waiting_runs"


def _roundtrip(broker, task, args=None, kwargs=None, timelimit=None):
    queue = f"wirecap-{uuid.uuid4().hex[:8]}"
    task_id = broker.publish(task, args, kwargs, queue=queue, timelimit=timelimit)
    envelope = broker.read_last(queue)
    assert envelope is not None, f"published {task} vanished from side queue"
    assert broker.queue_length(queue) == 0
    return task_id, envelope, queue


def _assert_delay_envelope(envelope, task, task_id, *, queue="celery"):
    assert envelope["content-type"] == "application/json"
    assert envelope["content-encoding"] == "utf-8"
    headers = envelope["headers"]
    assert headers["task"] == task
    assert headers["id"] == task_id == headers["root_id"] == envelope["properties"]["correlation_id"]
    assert headers["lang"] == "py"
    # Retry parity: a fresh delay carries no retry state. (That the worker
    # never requeues on failure is pinned by the execution tests.)
    assert headers["retries"] == 0
    # ETA parity: delay() means immediate delivery — no eta, no countdown.
    assert headers["eta"] is None
    assert headers["expires"] is None
    assert envelope["properties"]["delivery_info"] == {"exchange": "", "routing_key": queue}
    assert envelope["properties"]["body_encoding"] == "base64"
    assert envelope["properties"]["delivery_mode"] == 2
    body = json.loads(base64.b64decode(envelope["body"]))
    assert set(body[2]) == {"callbacks", "errbacks", "chain", "chord"}
    assert body[2] == {"callbacks": None, "errbacks": None, "chain": None, "chord": None}
    return headers, body


def test_run_task_envelope_shape(broker):
    """``run_cloud_agent.delay(run_id)`` wire shape: positional UUID arg,
    hard/soft time limits, no ETA."""
    run_id = str(uuid.uuid4())
    # Values mirror the settings-backed limits; only their presence and
    # order ([hard, soft], hard >= soft > 0) are pinned, not the numbers.
    task_id, envelope, queue = _roundtrip(broker, RUN_TASK, [run_id], timelimit=[330, 300])
    headers, body = _assert_delay_envelope(envelope, RUN_TASK, task_id, queue=queue)
    assert body[0] == [run_id]
    assert body[1] == {}
    hard, soft = headers["timelimit"]
    assert isinstance(hard, int) and isinstance(soft, int) and hard >= soft > 0
    assert headers["argsrepr"] == repr([run_id])
    assert headers["kwargsrepr"] == "{}"


@pytest.mark.parametrize(
    "task",
    [SCAN_TASK, SWEEP_TASK, EXPIRE_TASK],
    ids=["scan_queued_runs", "sweep_stale_runs", "expire_waiting_runs"],
)
def test_sweep_envelopes_carry_no_args_limits_or_eta(broker, task):
    """Beat-style invocation shape: no args, no time limits, no ETA."""
    task_id, envelope, queue = _roundtrip(broker, task, [])
    headers, body = _assert_delay_envelope(envelope, task, task_id, queue=queue)
    assert body[0] == []
    assert body[1] == {}
    assert headers["timelimit"] == [None, None]
    assert headers["argsrepr"] == "[]"


def test_broker_is_reachable(broker):
    """Preflight: the broker answers. Without it every execution test below
    would time out instead of reporting the environment problem."""
    assert broker.queue_length("celery") >= 0
