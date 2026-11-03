"""Offline-runner recovery: sweep row effects (PIDASHCONV-1063).

Task-oracle port of the sweep half of Django's
``tests/unit/runner/test_offline_runner_recovery.py`` (``70d684af``):
publish ``runner.release_pins_on_offline_runners`` /
``runner.fail_runs_on_offline_runners``, let the worker run, diff the
database before and after.

Backend selection: ``PIDASH_CONTRACT_BACKEND=django`` (default)
publishes Celery wire v2 to the Redis broker (a Django celery worker
must be running); ``=rust`` seeds ``rust_job_queue`` (``pidash-api
worker`` must be running against the same ``DATABASE_URL``).

Local-gated until PIDASHCONV-1055 merges main (see
``TASK_ORACLE_IGNORES.txt``): the matrix serves Django from the
frozen checkout, whose worker never heard of these tasks.
"""

from __future__ import annotations

import os

import pytest

from _harness import rust_queue
from _harness.celery import Broker, get_broker_url
from _harness.db import wait_for_condition

pytestmark = pytest.mark.contract

BACKEND = os.environ.get("PIDASH_CONTRACT_BACKEND", "django")

RELEASE_TASK = "runner.release_pins_on_offline_runners"
FAIL_TASK = "runner.fail_runs_on_offline_runners"

RUNNER_OFFLINE = "runner_offline"
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
    """Publish one sweep invocation; block until the worker settles it."""
    if BACKEND == "rust":
        celery_id = rust_queue.publish(task, [], {})
        rust_queue.wait_for_settled(celery_id, what=f"{task} settled")
        return
    broker.publish(task, [])
    # No broker-side settle signal on the Django path: the tests poll
    # the row effects below.


def _set_heartbeat(seeder, runner_id, sql):
    seeder.db.execute(
        f"UPDATE runner SET last_heartbeat_at = {sql} WHERE id = %s", (runner_id,)
    )


def _set_run_times(seeder, run_id, created_sql, assigned_sql=None):
    seeder.db.execute(
        "UPDATE agent_run SET created_at = %s WHERE id = %s" % (created_sql, "%s"),
        (run_id,),
    )
    if assigned_sql is not None:
        seeder.db.execute(
            "UPDATE agent_run SET assigned_at = %s WHERE id = %s" % (assigned_sql, "%s"),
            (run_id,),
        )


def _run_row(seeder, run_id):
    return seeder.db.fetchone(
        "SELECT status, runner_id, pinned_runner_id, "
        "COALESCE(error_details ->> 'message', '') AS error, "
        "COALESCE(error_details ->> 'code', '') AS error_code, "
        "ended_at, thread_id FROM agent_run WHERE id = %s",
        (run_id,),
    )


def _set_runner_status(seeder, runner_id, status):
    seeder.db.execute("UPDATE runner SET status = %s WHERE id = %s", (status, runner_id))


def _mark_managed(seeder, run_id):
    seeder.db.execute(
        "UPDATE agent_run SET executor_kind = 'managed_runner' WHERE id = %s", (run_id,)
    )


def test_release_unpins_stale_and_clears_parent_thread(
    seeder, daemon_world, machine_flow, broker
):
    """Stale pin: QUEUED run back to the queue, parent thread cleared."""
    parent = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    seeder.db.execute(
        "UPDATE agent_run SET thread_id = %s WHERE id = %s", ("parent-thread", parent["id"])
    )
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET pinned_runner_id = %s, parent_run_id = %s, "
        "runner_id = NULL WHERE id = %s",
        (machine_flow["runner_id"], parent["id"], run["id"]),
    )
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '10 minutes'")
    _set_run_times(seeder, run["id"], "now() - interval '10 minutes'")

    _fire(RELEASE_TASK, broker)

    def released():
        row = _run_row(seeder, run["id"])
        return row if row["pinned_runner_id"] is None else None

    wait_for_condition(released, timeout=TIMEOUT, what="pin released")
    assert _run_row(seeder, parent["id"])["thread_id"] == ""


def test_release_skips_fresh_heartbeat(seeder, daemon_world, machine_flow, broker):
    """A runner that heartbeats inside the grace keeps its pins."""
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET pinned_runner_id = %s, runner_id = NULL WHERE id = %s",
        (machine_flow["runner_id"], run["id"]),
    )
    _set_heartbeat(seeder, machine_flow["runner_id"], "now()")
    _set_run_times(seeder, run["id"], "now() - interval '10 minutes'")

    _fire(RELEASE_TASK, broker)

    import time

    time.sleep(5)
    assert str(_run_row(seeder, run["id"])["pinned_runner_id"]) == machine_flow["runner_id"]


def test_release_skips_young_run(seeder, daemon_world, machine_flow, broker):
    """A freshly created pin is not stale, however silent the runner."""
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET pinned_runner_id = %s, runner_id = NULL WHERE id = %s",
        (machine_flow["runner_id"], run["id"]),
    )
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '10 minutes'")

    _fire(RELEASE_TASK, broker)

    import time

    time.sleep(5)
    assert str(_run_row(seeder, run["id"])["pinned_runner_id"]) == machine_flow["runner_id"]


def test_release_hands_run_to_another_runner(seeder, daemon_world, machine_flow, broker):
    """The pod drain after a release assigns the run to a live runner."""
    alive = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}xalive",
    )
    _set_heartbeat(seeder, alive["id"], "now()")
    seeder.db.execute("UPDATE runner SET status = 'online' WHERE id = %s", (alive["id"],))
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET pinned_runner_id = %s, runner_id = NULL WHERE id = %s",
        (machine_flow["runner_id"], run["id"]),
    )
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '10 minutes'")
    _set_run_times(seeder, run["id"], "now() - interval '10 minutes'")

    _fire(RELEASE_TASK, broker)

    def handed_off():
        row = _run_row(seeder, run["id"])
        if row["status"] == "assigned" and str(row["runner_id"]) == alive["id"]:
            return row
        return None

    wait_for_condition(handed_off, timeout=TIMEOUT, what="run handed to live runner")


def test_release_drains_every_affected_pod(seeder, daemon_world, machine_flow, broker):
    """Two pods, two stale pins: both pods drain (no failure injection
    in the oracle — Django's patched-drain case is a unit concern)."""
    second_pod = seeder.create_pod(
        daemon_world["workspace"]["id"],
        daemon_world["project"]["id"],
        name="Contract pod two",
        is_default=False,
    )
    run_ids = []
    for pod_id in (daemon_world["pod"]["id"], second_pod["id"]):
        run = seeder.create_daemon_run(
            daemon_world["owner"]["id"],
            daemon_world["workspace"]["id"],
            pod_id,
            machine_flow["runner_id"],
            status="queued",
        )
        seeder.db.execute(
            "UPDATE agent_run SET pinned_runner_id = %s, runner_id = NULL WHERE id = %s",
            (machine_flow["runner_id"], run["id"]),
        )
        _set_run_times(seeder, run["id"], "now() - interval '10 minutes'")
        run_ids.append(run["id"])
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '10 minutes'")

    _fire(RELEASE_TASK, broker)

    def all_released():
        rows = [_run_row(seeder, run_id) for run_id in run_ids]
        return rows if all(row["pinned_runner_id"] is None for row in rows) else None

    wait_for_condition(all_released, timeout=TIMEOUT, what="both pods released")


def test_fail_finishes_stale_busy_runs(seeder, daemon_world, machine_flow, broker):
    """BUSY runs on a long-silent runner terminate with the offline stamp."""
    stale = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    stopping = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="cancel_requested",
    )
    young = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '2 hours'")
    _set_run_times(
        seeder, stale["id"], "now() - interval '2 hours'", "now() - interval '2 hours'"
    )
    _set_run_times(
        seeder, stopping["id"], "now() - interval '2 hours'", "now() - interval '2 hours'"
    )

    _fire(FAIL_TASK, broker)

    def finished():
        row = _run_row(seeder, stale["id"])
        return row if row["status"] == "failed" else None

    row = wait_for_condition(finished, timeout=TIMEOUT, what="stale run failed")
    assert row["error_code"] == RUNNER_OFFLINE
    assert row["error"].startswith("runner offline: no heartbeat")
    assert row["ended_at"] is not None

    stopped = _run_row(seeder, stopping["id"])
    assert stopped["status"] == "cancelled"
    assert stopped["error_code"] == RUNNER_OFFLINE
    assert _run_row(seeder, young["id"])["status"] == "running"


def test_fail_skips_live_runner(seeder, daemon_world, machine_flow, broker):
    """BUSY runs on a heartbeating runner are untouched."""
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    _set_run_times(seeder, run["id"], "now() - interval '2 hours'", "now() - interval '2 hours'")
    _set_heartbeat(seeder, machine_flow["runner_id"], "now()")

    _fire(FAIL_TASK, broker)

    import time

    time.sleep(5)
    assert _run_row(seeder, run["id"])["status"] == "running"


def test_release_unpins_never_heartbeated(seeder, daemon_world, machine_flow, broker):
    """A NULL heartbeat counts as silent: the pin is released."""
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET pinned_runner_id = %s, runner_id = NULL WHERE id = %s",
        (machine_flow["runner_id"], run["id"]),
    )
    _set_runner_status(seeder, machine_flow["runner_id"], "offline")
    _set_heartbeat(seeder, machine_flow["runner_id"], "NULL")
    _set_run_times(seeder, run["id"], "now() - interval '10 minutes'")

    _fire(RELEASE_TASK, broker)

    def released():
        row = _run_row(seeder, run["id"])
        return row if row["pinned_runner_id"] is None else None

    wait_for_condition(released, timeout=TIMEOUT, what="pin released")


def test_release_skips_managed_run(seeder, daemon_world, machine_flow, broker):
    """Managed-executor runs keep their pins; a local control proves the sweep ran."""
    managed = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET pinned_runner_id = %s, runner_id = NULL WHERE id = %s",
        (machine_flow["runner_id"], managed["id"]),
    )
    _mark_managed(seeder, managed["id"])
    _set_run_times(seeder, managed["id"], "now() - interval '10 minutes'")
    control = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET pinned_runner_id = %s, runner_id = NULL WHERE id = %s",
        (machine_flow["runner_id"], control["id"]),
    )
    _set_run_times(seeder, control["id"], "now() - interval '10 minutes'")
    _set_runner_status(seeder, machine_flow["runner_id"], "offline")
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '10 minutes'")

    _fire(RELEASE_TASK, broker)

    def control_released():
        row = _run_row(seeder, control["id"])
        return row if row["pinned_runner_id"] is None else None

    wait_for_condition(control_released, timeout=TIMEOUT, what="control pin released")
    assert (
        str(_run_row(seeder, managed["id"])["pinned_runner_id"]) == machine_flow["runner_id"]
    )


def test_release_waits_out_grace(seeder, daemon_world, machine_flow, broker):
    """120s of silence is under the 300s grace: the pin stays (Django's blip case)."""
    blip = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET pinned_runner_id = %s, runner_id = NULL WHERE id = %s",
        (machine_flow["runner_id"], blip["id"]),
    )
    _set_run_times(seeder, blip["id"], "now() - interval '10 minutes'")
    _set_runner_status(seeder, machine_flow["runner_id"], "offline")
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '120 seconds'")
    gone = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}xgone",
    )
    _set_runner_status(seeder, gone["id"], "offline")
    _set_heartbeat(seeder, gone["id"], "now() - interval '10 minutes'")
    control = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        gone["id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET pinned_runner_id = %s, runner_id = NULL WHERE id = %s",
        (gone["id"], control["id"]),
    )
    _set_run_times(seeder, control["id"], "now() - interval '10 minutes'")

    _fire(RELEASE_TASK, broker)

    def control_released():
        row = _run_row(seeder, control["id"])
        return row if row["pinned_runner_id"] is None else None

    wait_for_condition(control_released, timeout=TIMEOUT, what="control pin released")
    assert str(_run_row(seeder, blip["id"])["pinned_runner_id"]) == machine_flow["runner_id"]


def test_fail_finishes_never_heartbeated(seeder, daemon_world, machine_flow, broker):
    """A NULL heartbeat counts as silent: the BUSY run is failed with the stamp."""
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="assigned",
    )
    _set_run_times(seeder, run["id"], "now() - interval '2 hours'", "now() - interval '2 hours'")
    _set_runner_status(seeder, machine_flow["runner_id"], "offline")
    _set_heartbeat(seeder, machine_flow["runner_id"], "NULL")

    _fire(FAIL_TASK, broker)

    def finished():
        row = _run_row(seeder, run["id"])
        return row if row["status"] == "failed" else None

    row = wait_for_condition(finished, timeout=TIMEOUT, what="stale run failed")
    assert row["error_code"] == RUNNER_OFFLINE
    assert row["ended_at"] is not None


def test_fail_skips_managed_run(seeder, daemon_world, machine_flow, broker):
    """Managed-executor runs keep running; a local control proves the sweep ran."""
    managed = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    _mark_managed(seeder, managed["id"])
    _set_run_times(
        seeder, managed["id"], "now() - interval '2 hours'", "now() - interval '2 hours'"
    )
    control = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    _set_run_times(
        seeder, control["id"], "now() - interval '2 hours'", "now() - interval '2 hours'"
    )
    _set_runner_status(seeder, machine_flow["runner_id"], "offline")
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '2 hours'")

    _fire(FAIL_TASK, broker)

    def control_finished():
        row = _run_row(seeder, control["id"])
        return row if row["status"] == "failed" else None

    wait_for_condition(control_finished, timeout=TIMEOUT, what="control run failed")
    assert _run_row(seeder, managed["id"])["status"] == "running"


def test_fail_frees_pod_queue_for_live_runner(seeder, daemon_world, machine_flow, broker):
    """The post-sweep drain hands a waiting queued run to a live runner."""
    alive = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}xalive",
    )
    _set_heartbeat(seeder, alive["id"], "now()")
    seeder.db.execute("UPDATE runner SET status = 'online' WHERE id = %s", (alive["id"],))
    stranded = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    _set_run_times(
        seeder, stranded["id"], "now() - interval '2 hours'", "now() - interval '2 hours'"
    )
    queued = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute("UPDATE agent_run SET runner_id = NULL WHERE id = %s", (queued["id"],))
    _set_runner_status(seeder, machine_flow["runner_id"], "offline")
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '2 hours'")

    _fire(FAIL_TASK, broker)

    def stranded_failed():
        row = _run_row(seeder, stranded["id"])
        return row if row["status"] == "failed" else None

    wait_for_condition(stranded_failed, timeout=TIMEOUT, what="stranded run failed")

    def handed_off():
        row = _run_row(seeder, queued["id"])
        if row["status"] == "assigned" and str(row["runner_id"]) == alive["id"]:
            return row
        return None

    wait_for_condition(handed_off, timeout=TIMEOUT, what="queued run handed to live runner")


def test_fail_waits_out_grace(seeder, daemon_world, machine_flow, broker):
    """45min of silence is under the 1h grace: a sleeping laptop keeps its run."""
    asleep = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    _set_run_times(
        seeder, asleep["id"], "now() - interval '2 hours'", "now() - interval '2 hours'"
    )
    _set_runner_status(seeder, machine_flow["runner_id"], "offline")
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '2700 seconds'")
    gone = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}xgone",
    )
    _set_runner_status(seeder, gone["id"], "offline")
    _set_heartbeat(seeder, gone["id"], "now() - interval '2 hours'")
    control = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        gone["id"],
        status="running",
    )
    _set_run_times(
        seeder, control["id"], "now() - interval '2 hours'", "now() - interval '2 hours'"
    )

    _fire(FAIL_TASK, broker)

    def control_finished():
        row = _run_row(seeder, control["id"])
        return row if row["status"] == "failed" else None

    wait_for_condition(control_finished, timeout=TIMEOUT, what="control run failed")
    assert _run_row(seeder, asleep["id"])["status"] == "running"


@pytest.mark.parametrize("status", ["assigned", "awaiting_approval", "awaiting_reauth"])
def test_fail_finishes_busy_statuses(seeder, daemon_world, machine_flow, broker, status):
    """Every non-running BUSY status terminates with the offline stamp."""
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status=status,
    )
    _set_run_times(seeder, run["id"], "now() - interval '2 hours'", "now() - interval '2 hours'")
    _set_runner_status(seeder, machine_flow["runner_id"], "offline")
    _set_heartbeat(seeder, machine_flow["runner_id"], "now() - interval '2 hours'")

    _fire(FAIL_TASK, broker)

    def finished():
        row = _run_row(seeder, run["id"])
        return row if row["status"] == "failed" else None

    row = wait_for_condition(finished, timeout=TIMEOUT, what=f"{status} run failed")
    assert row["error_code"] == RUNNER_OFFLINE
    assert row["ended_at"] is not None
