"""Oracle: ``cloud_agent.sweep_stale_runs`` (PIDASHCONV-22, D-11).

Source: ``apps/api/pi_dash/cloud_agent/tasks.py`` ``sweep_stale_runs`` —
RUNNING cloud rows older than ``HARD_LIMIT + STALE_GRACE`` (defaults
330 + 60) are ``FAILED``/``run_timeout``, except cancel-requested rows
which go ``CANCELLED``; everything else is untouched
(first-writer-wins: terminal rows stay terminal).

``started_ago_seconds`` uses two hours so the stale cases hold for any sane
limit override, not just the 390s default.
"""

import pytest

from .conftest import (
    CANCELLED,
    CLOUD_AGENT,
    COMPLETED,
    FAILED,
    LOCAL_RUNNER,
    RUNNING,
    create_agent_run,
    get_run,
    run_events,
    wait_for_queue_drain,
    wait_for_run,
)

pytestmark = pytest.mark.contract

SWEEP_TASK = "cloud_agent.sweep_stale_runs"
TWO_HOURS = 2 * 3600


def _publish_sweep(broker):
    baseline = broker.queue_length()
    broker.publish(SWEEP_TASK, [])
    return baseline


def test_stale_running_run_times_out(db, broker, seeder, world):
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        status=RUNNING, started_ago_seconds=TWO_HOURS,
    )
    baseline = _publish_sweep(broker)
    after = wait_for_run(db, run["id"], status=FAILED)
    assert after["error_code"] == "run_timeout"
    kinds = [row["kind"] for row in run_events(db, run["id"])]
    assert "terminal" in kinds
    wait_for_queue_drain(broker, baseline=baseline)


def test_cancel_requested_stale_run_is_cancelled(db, broker, seeder, world):
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        status=RUNNING, started_ago_seconds=TWO_HOURS,
        cancel_reason="stop it",
    )
    _publish_sweep(broker)
    after = wait_for_run(db, run["id"], status=CANCELLED)
    assert after["error_code"] == "cancelled"
    assert after["error"] == "stop it"


def test_fresh_running_run_is_untouched(db, broker, seeder, world):
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        status=RUNNING,
    )
    baseline = _publish_sweep(broker)
    wait_for_queue_drain(broker, baseline=baseline)
    after = get_run(db, run["id"])
    assert after["status"] == RUNNING
    assert after["error_code"] == ""
    assert run_events(db, run["id"]) == []


def test_other_executors_are_untouched(db, broker, seeder, world):
    """The sweep is cloud-scoped: an ancient RUNNING local run survives."""
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        executor_kind=LOCAL_RUNNER, status=RUNNING, started_ago_seconds=TWO_HOURS,
    )
    baseline = _publish_sweep(broker)
    wait_for_queue_drain(broker, baseline=baseline)
    assert get_run(db, run["id"])["status"] == RUNNING


def test_terminal_run_is_untouched(db, broker, seeder, world):
    """First-writer-wins: an old terminal row gains no second terminal event."""
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        status=COMPLETED, started_ago_seconds=TWO_HOURS,
    )
    baseline = _publish_sweep(broker)
    wait_for_queue_drain(broker, baseline=baseline)
    after = get_run(db, run["id"])
    assert after["status"] == COMPLETED
    assert run_events(db, run["id"]) == []
