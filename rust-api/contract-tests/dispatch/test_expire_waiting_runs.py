"""Oracle: ``managed_runner.expire_waiting_runs`` (PIDASHCONV-22, D-11).

Source: ``apps/api/pi_dash/managed_runner/tasks.py`` — QUEUED managed rows
older than ``MANAGED_RUNNER_QUEUED_MAX_AGE_SECS`` (default 43200) are
finalized ``FAILED`` / ``desktop_not_connected`` with the "never came
online" detail, first 500, guarded on ``QUEUED``; recent waits, other
executors and non-QUEUED rows are untouched.

History: this oracle first pinned a dead letter — no installed Django app
imported ``pi_dash.managed_runner.tasks``, so a stock worker discarded the
beat publication and stale waits never expired. PDASHOSS01-224 registers
the task (``runner/tasks.py`` imports it next to ``cloud_agent.tasks``),
and the Rust worker has executed it since PIDASHCONV-832, so both backends
now run the sweep. Django checkouts that predate the fix (the frozen
rust-dev tree) register it through ``_harness/oracle_worker_includes.txt``.

Backend selection: ``PIDASH_CONTRACT_BACKEND=django`` (default) publishes
Celery wire v2 to the Redis broker (a Django celery worker must be
running); ``=rust`` seeds ``rust_job_queue`` (``pidash-api worker`` must be
running against the same ``DATABASE_URL``).

``created_ago_seconds`` uses thirty days so the stale case holds for any
sane ``MANAGED_RUNNER_QUEUED_MAX_AGE_SECS`` override.
"""

import os

import pytest

from _harness import rust_queue

from .conftest import (
    CANCELLED,
    FAILED,
    LOCAL_RUNNER,
    MANAGED_RUNNER,
    QUEUED,
    create_agent_run,
    get_run,
    run_events,
    wait_for_queue_drain,
    wait_for_run,
)

pytestmark = pytest.mark.contract

BACKEND = os.environ.get("PIDASH_CONTRACT_BACKEND", "django")

EXPIRE_TASK = "managed_runner.expire_waiting_runs"
THIRTY_DAYS = 30 * 24 * 3600

NOT_CONNECTED = "desktop_not_connected"
NEVER_CAME_ONLINE = (
    "Pi Dash Agent never came online for this run. "
    "Open the desktop app and run the issue again."
)


def _sweep(broker):
    """Publish one sweep exactly as beat does and wait until it settled."""
    if BACKEND == "rust":
        celery_id = rust_queue.publish(EXPIRE_TASK, [], {})
        rust_queue.wait_for_settled(celery_id, what=f"{EXPIRE_TASK} settled")
        return
    baseline = broker.queue_length()
    broker.publish(EXPIRE_TASK, [])
    wait_for_queue_drain(broker, baseline=baseline)


def _managed_wait(seeder, world, **kwargs):
    return create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        executor_kind=MANAGED_RUNNER, **kwargs,
    )


def test_sweep_fails_a_wait_that_outlived_the_bound(db, broker, seeder, world):
    run = _managed_wait(seeder, world, created_ago_seconds=THIRTY_DAYS)
    assert get_run(db, run["id"])["status"] == QUEUED
    _sweep(broker)
    after = wait_for_run(db, run["id"], status=FAILED)
    assert after["error_code"] == NOT_CONNECTED
    assert after["error"] == NEVER_CAME_ONLINE
    assert after["ended_at"] is not None
    # The terminal event is cloud-only (``agent_run_finalization``).
    assert run_events(db, run["id"]) == []


def test_sweep_leaves_a_recent_wait_alone(db, broker, seeder, world):
    run = _managed_wait(seeder, world)
    before = get_run(db, run["id"])
    _sweep(broker)
    assert get_run(db, run["id"]) == before
    assert run_events(db, run["id"]) == []


def test_sweep_ignores_other_executors_and_non_queued_rows(db, broker, seeder, world):
    """Only managed QUEUED rows are eligible: an equally ancient local
    wait and an already-terminal managed row survive byte-identical. (A
    local row, not a cloud one: the Rust worker's own beat fires the cloud
    scan, which would expire an ancient cloud wait mid-test.)"""
    local = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        executor_kind=LOCAL_RUNNER, created_ago_seconds=THIRTY_DAYS,
    )
    cancelled = _managed_wait(
        seeder, world, status=CANCELLED, created_ago_seconds=THIRTY_DAYS
    )
    before = {rid: get_run(db, rid) for rid in (local["id"], cancelled["id"])}
    # A stale managed wait in the same sweep proves the sweep actually ran.
    stale = _managed_wait(seeder, world, created_ago_seconds=THIRTY_DAYS)
    _sweep(broker)
    wait_for_run(db, stale["id"], status=FAILED)
    for rid, row in before.items():
        assert get_run(db, rid) == row
