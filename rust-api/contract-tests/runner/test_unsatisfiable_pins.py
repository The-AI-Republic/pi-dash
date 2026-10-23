"""Unsatisfiable-pin recovery + BUSY-but-dead sweep: row effects (PDASHOSS01-233).

Task-oracle port of Django's ``tests/unit/runner/test_unsatisfiable_pins.py``
(PR #541): publish ``runner.reconcile_unsatisfiable_pins`` /
``runner.mark_offline_runners`` / ``runner.sweep_stale_runners``, let the
worker run, diff the database before and after.

Backend selection: ``PIDASH_CONTRACT_BACKEND=django`` (default)
publishes Celery wire v2 to the Redis broker (a Django celery worker
must be running); ``=rust`` seeds ``rust_job_queue`` (``pidash-api
worker`` must be running against the same ``DATABASE_URL``).

Both workers run with the default ``RUNNER_PIN_AUTO_RELEASE_SECS``
(600s); the ``<= 0`` disable switch is a per-process setting and stays
a unit concern on each side.

Local-gated until PR #541 reaches rust-dev (see
``TASK_ORACLE_IGNORES.txt``): the matrix serves Django from the frozen
checkout, whose worker never heard of the reconcile task.
"""

from __future__ import annotations

import os
import time

import pytest

from _harness import rust_queue
from _harness.celery import Broker, get_broker_url
from _harness.db import wait_for_condition

pytestmark = pytest.mark.contract

BACKEND = os.environ.get("PIDASH_CONTRACT_BACKEND", "django")

RECONCILE_TASK = "runner.reconcile_unsatisfiable_pins"
MARK_OFFLINE_TASK = "runner.mark_offline_runners"
SWEEP_STALE_TASK = "runner.sweep_stale_runners"
SWEEPERS = (MARK_OFFLINE_TASK, SWEEP_STALE_TASK)

PAST_BOUND = "now() - interval '601 seconds'"
STALE = "now() - interval '10 minutes'"
TIMEOUT = 60.0


@pytest.fixture()
def broker():
    if BACKEND == "rust":
        yield None
        return
    broker = Broker(get_broker_url())
    yield broker
    broker.close()


def _fire(task, broker):
    """Publish one task invocation; on Rust block until it settles."""
    if BACKEND == "rust":
        celery_id = rust_queue.publish(task, [], {})
        rust_queue.wait_for_settled(celery_id, what=f"{task} settled")
        return
    broker.publish(task, [])


def _fire_and_settle(task, broker):
    """For negative assertions: Rust settles synchronously; the Django
    path has no settle signal, so give the worker a fixed window (the
    offline-sweeps suite precedent)."""
    _fire(task, broker)
    if BACKEND != "rust":
        time.sleep(5)


def _runner(seeder, daemon_world, suffix, *, status, heartbeat_sql):
    runner = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}x{suffix}",
    )
    seeder.db.execute(
        f"UPDATE runner SET status = %s, last_heartbeat_at = {heartbeat_sql} WHERE id = %s",
        (status, runner["id"]),
    )
    return runner["id"]


def _set_runner(seeder, runner_id, *, status, heartbeat_sql):
    seeder.db.execute(
        f"UPDATE runner SET status = %s, last_heartbeat_at = {heartbeat_sql} WHERE id = %s",
        (status, runner_id),
    )


def _pinned_run(seeder, daemon_world, runner_id, *, created_sql="now()", parent_id=None):
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        runner_id,
        status="queued",
    )
    seeder.db.execute(
        f"UPDATE agent_run SET pinned_runner_id = %s, runner_id = NULL, parent_run_id = %s, "
        f"created_at = {created_sql} WHERE id = %s",
        (runner_id, parent_id, run["id"]),
    )
    return run["id"]


def _active_run(seeder, daemon_world, runner_id, status="running"):
    return seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        runner_id,
        status=status,
    )["id"]


def _run_row(seeder, run_id):
    return seeder.db.fetchone(
        "SELECT status, runner_id, pinned_runner_id, thread_id FROM agent_run WHERE id = %s",
        (run_id,),
    )


def _runner_status(seeder, runner_id):
    return seeder.db.fetchone("SELECT status FROM runner WHERE id = %s", (runner_id,))["status"]


# ---------------------------------------------------------------------------
# (1) Wedged-busy pinned runner: released, pin preserved
# ---------------------------------------------------------------------------


def test_wedged_busy_pin_released_and_served(seeder, daemon_world, machine_flow, broker):
    """BUSY + fresh heartbeat + no busy-status run: the runner goes
    online and its own pinned run is assigned to it, pin kept."""
    runner_id = machine_flow["runner_id"]
    _set_runner(seeder, runner_id, status="busy", heartbeat_sql="now()")
    run_id = _pinned_run(seeder, daemon_world, runner_id)

    _fire(RECONCILE_TASK, broker)

    def served():
        row = _run_row(seeder, run_id)
        return row if row["status"] == "assigned" else None

    row = wait_for_condition(served, timeout=TIMEOUT, what="pinned run served")
    assert str(row["runner_id"]) == runner_id
    assert str(row["pinned_runner_id"]) == runner_id


def test_wedged_busy_dead_runner_not_resurrected(seeder, daemon_world, machine_flow, broker):
    """A wedged runner with a stale heartbeat stays busy: the sweeper,
    not the pin reconcile, owns it (flips it offline)."""
    runner_id = machine_flow["runner_id"]
    _set_runner(seeder, runner_id, status="busy", heartbeat_sql=STALE)
    run_id = _pinned_run(seeder, daemon_world, runner_id)

    _fire_and_settle(RECONCILE_TASK, broker)

    assert _runner_status(seeder, runner_id) == "busy"
    row = _run_row(seeder, run_id)
    assert row["status"] == "queued"
    assert str(row["pinned_runner_id"]) == runner_id


def test_busy_runner_with_active_run_not_released(seeder, daemon_world, machine_flow, broker):
    runner_id = machine_flow["runner_id"]
    _set_runner(seeder, runner_id, status="busy", heartbeat_sql="now()")
    _active_run(seeder, daemon_world, runner_id)
    run_id = _pinned_run(seeder, daemon_world, runner_id)

    _fire_and_settle(RECONCILE_TASK, broker)

    assert _runner_status(seeder, runner_id) == "busy"
    row = _run_row(seeder, run_id)
    assert row["status"] == "queued"
    assert str(row["pinned_runner_id"]) == runner_id


# ---------------------------------------------------------------------------
# (2) Bounded pin auto-release
# ---------------------------------------------------------------------------


def test_pin_past_bound_released_and_assigned_elsewhere(
    seeder, daemon_world, machine_flow, broker
):
    """Pin past the bound to an offline runner: pin cleared, parent's
    resume hint dropped, run assigned to the healthy runner."""
    dead = machine_flow["runner_id"]
    _set_runner(seeder, dead, status="offline", heartbeat_sql=STALE)
    healthy = _runner(seeder, daemon_world, "healthy", status="online", heartbeat_sql="now()")
    parent_id = _active_run(seeder, daemon_world, dead, status="completed")
    seeder.db.execute(
        "UPDATE agent_run SET thread_id = 'parent-thread' WHERE id = %s", (parent_id,)
    )
    run_id = _pinned_run(
        seeder, daemon_world, dead, created_sql=PAST_BOUND, parent_id=parent_id
    )

    _fire(RECONCILE_TASK, broker)

    def handed_off():
        row = _run_row(seeder, run_id)
        return row if row["status"] == "assigned" else None

    row = wait_for_condition(handed_off, timeout=TIMEOUT, what="run assigned elsewhere")
    assert row["pinned_runner_id"] is None
    assert str(row["runner_id"]) == healthy
    assert _run_row(seeder, parent_id)["thread_id"] == ""


def test_pin_within_bound_kept(seeder, daemon_world, machine_flow, broker):
    dead = machine_flow["runner_id"]
    _set_runner(seeder, dead, status="offline", heartbeat_sql=STALE)
    _runner(seeder, daemon_world, "healthy", status="online", heartbeat_sql="now()")
    run_id = _pinned_run(seeder, daemon_world, dead, created_sql="now() - interval '30 seconds'")

    _fire_and_settle(RECONCILE_TASK, broker)

    row = _run_row(seeder, run_id)
    assert row["status"] == "queued"
    assert str(row["pinned_runner_id"]) == dead


def test_pin_to_assignable_runner_kept_past_bound(seeder, daemon_world, machine_flow, broker):
    runner_id = machine_flow["runner_id"]
    _set_runner(seeder, runner_id, status="online", heartbeat_sql="now()")
    run_id = _pinned_run(seeder, daemon_world, runner_id, created_sql=PAST_BOUND)

    _fire_and_settle(RECONCILE_TASK, broker)

    assert str(_run_row(seeder, run_id)["pinned_runner_id"]) == runner_id


def test_pin_to_alive_working_runner_kept_past_bound(seeder, daemon_world, machine_flow, broker):
    runner_id = machine_flow["runner_id"]
    _set_runner(seeder, runner_id, status="busy", heartbeat_sql="now()")
    _active_run(seeder, daemon_world, runner_id)
    run_id = _pinned_run(seeder, daemon_world, runner_id, created_sql=PAST_BOUND)

    _fire_and_settle(RECONCILE_TASK, broker)

    row = _run_row(seeder, run_id)
    assert row["status"] == "queued"
    assert str(row["pinned_runner_id"]) == runner_id


def test_managed_runner_pin_never_cleared(seeder, daemon_world, machine_flow, broker):
    dead = machine_flow["runner_id"]
    _set_runner(seeder, dead, status="offline", heartbeat_sql=STALE)
    run_id = _pinned_run(seeder, daemon_world, dead, created_sql=PAST_BOUND)
    seeder.db.execute(
        "UPDATE agent_run SET executor_kind = 'managed_runner' WHERE id = %s", (run_id,)
    )

    _fire_and_settle(RECONCILE_TASK, broker)

    assert str(_run_row(seeder, run_id)["pinned_runner_id"]) == dead


# ---------------------------------------------------------------------------
# (3) BUSY-but-dead sweep — both sweepers
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("task", SWEEPERS)
def test_sweep_flips_dead_busy_runner_offline(seeder, daemon_world, machine_flow, broker, task):
    runner_id = machine_flow["runner_id"]
    _set_runner(seeder, runner_id, status="busy", heartbeat_sql=STALE)

    _fire(task, broker)

    wait_for_condition(
        lambda: _runner_status(seeder, runner_id) == "offline" or None,
        timeout=TIMEOUT,
        what="dead busy runner offline",
    )


@pytest.mark.parametrize("task", SWEEPERS)
def test_sweep_keeps_dead_busy_runner_with_active_run(
    seeder, daemon_world, machine_flow, broker, task
):
    runner_id = machine_flow["runner_id"]
    _set_runner(seeder, runner_id, status="busy", heartbeat_sql=STALE)
    _active_run(seeder, daemon_world, runner_id)

    _fire_and_settle(task, broker)

    assert _runner_status(seeder, runner_id) == "busy"


@pytest.mark.parametrize("task", SWEEPERS)
def test_sweep_keeps_alive_busy_runner(seeder, daemon_world, machine_flow, broker, task):
    runner_id = machine_flow["runner_id"]
    _set_runner(seeder, runner_id, status="busy", heartbeat_sql="now()")

    _fire_and_settle(task, broker)

    assert _runner_status(seeder, runner_id) == "busy"


def test_dead_busy_runner_with_old_pin_recovers_end_to_end(
    seeder, daemon_world, machine_flow, broker
):
    """The incident shape: a daemon dies mid-wedge. The sweeper flips it
    offline, the reconcile clears the old pin and a healthy runner takes
    the run — no manual release-pin."""
    dead = machine_flow["runner_id"]
    _set_runner(seeder, dead, status="busy", heartbeat_sql=STALE)
    healthy = _runner(seeder, daemon_world, "healthy", status="online", heartbeat_sql="now()")
    run_id = _pinned_run(seeder, daemon_world, dead, created_sql=PAST_BOUND)

    _fire(MARK_OFFLINE_TASK, broker)
    wait_for_condition(
        lambda: _runner_status(seeder, dead) == "offline" or None,
        timeout=TIMEOUT,
        what="dead busy runner offline",
    )
    _fire(RECONCILE_TASK, broker)

    def handed_off():
        row = _run_row(seeder, run_id)
        return row if row["status"] == "assigned" else None

    row = wait_for_condition(handed_off, timeout=TIMEOUT, what="run assigned elsewhere")
    assert row["pinned_runner_id"] is None
    assert str(row["runner_id"]) == healthy
