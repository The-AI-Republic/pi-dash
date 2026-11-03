"""Oracle: ``cloud_agent.scan_queued_runs`` (PIDASHCONV-22, D-11).

Source: ``apps/api/pi_dash/cloud_agent/tasks.py`` ``scan_queued_runs`` —
expires QUEUED rows older than ``CLOUD_AGENT_MAX_QUEUE_AGE_SECONDS``
(default 900) with ``dispatch_timeout``, then leases + publishes
``run_agent_run`` for waiting rows via ``dispatch_waiting``
(``cloud_agent/dispatch.py``: ``lease_expires_at = now + 60s``,
``dispatch_attempts + 1``, capacity ``MAX_RUNNING_PER_WORKSPACE`` = 2).

- ancient QUEUED → ``FAILED``/``dispatch_timeout`` with ``dispatch_attempts``
  still 0 (dispatch never touched it).
- fresh QUEUED + eligible member → end-to-end chain: lease, publish, claim,
  execute → ``FAILED``/``llm_config_missing`` with ``dispatch_attempts == 1``.
- future-leased QUEUED → left alone (not selected, not dispatched).
- full workspace (2 RUNNING) → QUEUED row untouched (capacity 0).

``created_ago_seconds`` uses two days so the expiry case holds for any sane
``MAX_QUEUE_AGE`` override, not just the 900s default.
"""

import pytest

from .conftest import (
    CLOUD_AGENT,
    FAILED,
    QUEUED,
    RUNNING,
    create_agent_run,
    get_run,
    wait_for_queue_drain,
    wait_for_run,
)

pytestmark = pytest.mark.contract

SCAN_TASK = "cloud_agent.scan_queued_runs"
TWO_DAYS = 2 * 24 * 3600


def _publish_scan(broker):
    baseline = broker.queue_length()
    broker.publish(SCAN_TASK, [])
    return baseline


def test_expires_ancient_queued_run(db, broker, seeder, world):
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        created_ago_seconds=TWO_DAYS,
    )
    baseline = _publish_scan(broker)
    after = wait_for_run(db, run["id"], status=FAILED)
    assert after["error_code"] == "dispatch_timeout"
    assert after["dispatch_attempts"] == 0
    wait_for_queue_drain(broker, baseline=baseline)


def test_dispatches_waiting_run_end_to_end(db, broker, seeder, world):
    """The full chain the beat entry exists for: scan → lease + publish →
    claim → execute. The seeded member has no LLM config, so execution ends
    ``llm_config_missing`` — the dispatch half is what this pins
    (``dispatch_attempts == 1`` proves exactly one offer)."""
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"]
    )
    baseline = _publish_scan(broker)
    after = wait_for_run(db, run["id"], status=FAILED)
    assert after["error_code"] == "llm_config_missing"
    assert after["dispatch_attempts"] == 1
    wait_for_queue_drain(broker, baseline=baseline)


def test_future_leased_run_is_left_alone(db, broker, seeder, world):
    """A QUEUED row with a future lease is not selectable and not offered:
    status, attempts and lease survive the scan."""
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        lease_in_seconds=3600,
    )
    before = get_run(db, run["id"])
    baseline = _publish_scan(broker)
    wait_for_queue_drain(broker, baseline=baseline)
    after = get_run(db, run["id"])
    assert after["status"] == QUEUED
    assert after["dispatch_attempts"] == 0
    assert after["lease_expires_at"] == before["lease_expires_at"]


def test_full_workspace_dispatches_nothing(db, broker, seeder, world):
    """At capacity (2 RUNNING, the ``MAX_RUNNING_PER_WORKSPACE`` default) the
    scan offers nothing: the QUEUED row keeps attempts 0 and no lease."""
    for _ in range(2):
        create_agent_run(
            seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
            status=RUNNING,
        )
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"]
    )
    baseline = _publish_scan(broker)
    wait_for_queue_drain(broker, baseline=baseline)
    after = get_run(db, run["id"])
    assert after["status"] == QUEUED
    assert after["dispatch_attempts"] == 0
    assert after["lease_expires_at"] is None
