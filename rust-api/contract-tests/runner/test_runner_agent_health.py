"""Activity-watchdog health plumbing (PIDASHCONV-1315).

Django main ``b790ee7fa`` ("passive-kill activity watchdog") added
``RunnerLiveState.agent_health`` / ``agent_last_activity_at`` and wired
them through four places:

* the daemon poll upsert stores them (an ``agent_health`` value outside
  ``AgentHealth.values`` keeps the known-good value),
* ``runner.reconcile_stalled_runs`` skips runs whose runner reports
  ``agent_health`` (those runners own the 24h/48h watchdog; the cloud's
  6-minute backstop only covers older runners),
* ``RunnerLiveStateSerializer`` exposes both keys,
* the issue-detail agent-run ``live_state`` exposes both keys (covered
  in ``app_issues/test_issue_agent_health.py``).

Backend selection: ``PIDASH_CONTRACT_BACKEND=django`` (default) runs
the sweep through the Celery broker (a Django worker must be
running); ``=rust`` seeds ``rust_job_queue`` (``pidash-api worker``
must be running against the same ``DATABASE_URL``). HTTP assertions
run against ``BASE_URL`` in both modes.
"""

from __future__ import annotations

import os
from datetime import datetime, timezone

import pytest

from _harness import rust_queue
from _harness.celery import Broker, get_broker_url
from _harness.db import wait_for_condition

from .conftest import DAEMON

pytestmark = pytest.mark.contract

BACKEND = os.environ.get("PIDASH_CONTRACT_BACKEND", "django")

SWEEP_TASK = "runner.reconcile_stalled_runs"
TIMEOUT = 60.0

WEB = "/api/runners"

EXPECTED_LIVE_STATE_KEYS = [
    "observed_run_id",
    "last_event_at",
    "last_event_kind",
    "last_event_summary",
    "agent_pid",
    "agent_subprocess_alive",
    "approvals_pending",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "usage",
    "llm_model",
    "turn_count",
    "agent_health",
    "agent_last_activity_at",
    "updated_at",
]


@pytest.fixture()
def broker():
    if BACKEND == "rust":
        yield None
        return
    broker = Broker(get_broker_url())
    yield broker
    broker.close()


@pytest.fixture()
def live_states(seeder):
    """Collect live-state runner ids (and wholly-owned extra runners).

    Django emulates ``CASCADE`` in the ORM collector, so the raw-SQL
    ``machine_flow`` teardown cannot delete a runner that owns a
    live-state row. This finalizer (set up after ``machine_flow``, so
    it tears down first) removes them in FK order.
    """
    ids: list[str] = []
    extra: list[str] = []
    yield {"live": ids, "extra": extra}
    for runner_id in ids:
        seeder.db.execute(
            "DELETE FROM runner_live_state WHERE runner_id = %s", (runner_id,)
        )
    for runner_id in extra:
        seeder.db.execute("DELETE FROM agent_run WHERE runner_id = %s", (runner_id,))
        seeder.db.execute("DELETE FROM runner WHERE id = %s", (runner_id,))


def _fire_sweep(broker):
    """Publish one stall-sweep invocation; block until the worker settles it."""
    if BACKEND == "rust":
        celery_id = rust_queue.publish(SWEEP_TASK, [], {})
        rust_queue.wait_for_settled(celery_id, what=f"{SWEEP_TASK} settled")
        return
    broker.publish(SWEEP_TASK, [])


def _seed_live_state(seeder, runner_id, run_id, *, agent_health, stale=True):
    """A fresh-snapshot live-state row with ``observed_run_id`` matching the run.

    ``updated_at`` fresh (runner still reporting); ``last_event_at``
    7 minutes stale (past the 360s threshold) unless ``stale=False``.
    """
    activity = (
        "'2026-10-20 01:02:03+00:00'::timestamptz" if agent_health else "NULL"
    )
    health = "'%s'" % agent_health if agent_health else "NULL"
    last_event = "now() - interval '7 minutes'" if stale else "now()"
    seeder.db.execute(
        "INSERT INTO runner_live_state "
        "(runner_id, observed_run_id, last_event_at, last_event_kind, "
        "agent_pid, agent_subprocess_alive, approvals_pending, usage, "
        "llm_model, turn_count, agent_health, agent_last_activity_at, "
        "updated_at) "
        "VALUES (%s, %s, " + last_event + ", 'tool', "
        "4242, true, 0, '{}', 'm', 3, "
        + health
        + ", "
        + activity
        + ", now())",
        (runner_id, run_id),
    )


def _seed_stale_run(seeder, daemon_world, machine_flow, *, agent_health):
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    _seed_live_state(seeder, machine_flow["runner_id"], run["id"], agent_health=agent_health)
    return run


def _run_status(seeder, run_id):
    row = seeder.db.fetchone("SELECT status FROM agent_run WHERE id = %s", (run_id,))
    return row["status"]


def test_stall_sweep_skips_healthy_watchdog_run(
    seeder, daemon_world, machine_flow, broker, live_states
):
    """A run whose runner reports agent_health is never failed for silence.

    The NULL-health control seeded alongside proves the sweep actually
    ran: it must reap while the healthy run survives the same pass.
    """
    healthy = _seed_stale_run(seeder, daemon_world, machine_flow, agent_health="healthy")
    live_states["live"].append(machine_flow["runner_id"])
    control = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    # The control needs its own runner: one live-state row per runner.
    other = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}healthctl",
    )
    seeder.db.execute(
        "UPDATE agent_run SET runner_id = %s WHERE id = %s", (other["id"], control["id"])
    )
    _seed_live_state(seeder, other["id"], control["id"], agent_health=None)
    live_states["live"].append(other["id"])
    live_states["extra"].append(other["id"])
    _fire_sweep(broker)
    if BACKEND == "django":
        # No broker-side settle signal: the control reap proves the pass ran.
        wait_for_condition(
            lambda: _run_status(seeder, control["id"]) == "failed",
            timeout=TIMEOUT,
            what="stale control run reaped",
        )
    assert _run_status(seeder, control["id"]) == "failed"
    assert _run_status(seeder, healthy["id"]) == "running"


def _open_runner_session(client, runner_id):
    response = client.post(f"{DAEMON}/runners/{runner_id}/sessions/", json={})
    assert response.status_code == 201, response.text
    return response.json()


def _post_poll(client, url, payload, attempts=5):
    """POST a poll, tolerating Django's async-Redis flake.

    Same transport-only tolerance as the sessions suite (only 500s
    are retried), with a pause between attempts: back-to-back polls
    can all land on the closed loop.
    """
    import time

    response = client.post(url, json=payload)
    for _ in range(attempts - 1):
        if response.status_code != 500:
            break
        time.sleep(2)
        response = client.post(url, json=payload)
    return response


def _live_state_row(seeder, runner_id):
    return seeder.db.fetchone(
        "SELECT agent_health, agent_last_activity_at, observed_run_id, turn_count"
        " FROM runner_live_state WHERE runner_id = %s",
        (runner_id,),
    )


def test_poll_stores_agent_health_and_activity(
    seeder, daemon_world, machine_flow, machine_client, live_states
):
    """A busy poll carrying the watchdog fields persists them on the row."""
    live_states["live"].append(machine_flow["runner_id"])
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    url = f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{opened['session_id']}/poll"
    response = _post_poll(
        machine_client,
        url,
        {
            "ack": [],
            "status": {
                "status": "busy",
                "in_flight_run": run["id"],
                "observed_run_id": run["id"],
                "agent_health": "healthy",
                "agent_last_activity_at": "2026-10-20T01:02:03Z",
                "turn_count": 5,
            },
        },
    )
    assert response.status_code == 200, response.text
    row = _live_state_row(seeder, machine_flow["runner_id"])
    assert row["agent_health"] == "healthy"
    assert row["agent_last_activity_at"] == datetime(2026, 10, 20, 1, 2, 3, tzinfo=timezone.utc)
    assert str(row["observed_run_id"]) == run["id"]
    assert row["turn_count"] == 5


def test_poll_keeps_known_good_health_on_unknown_label(
    seeder, daemon_world, machine_flow, machine_client, live_states
):
    """An agent_health label outside AgentHealth.values is skipped, not stored."""
    live_states["live"].append(machine_flow["runner_id"])
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    # Known-good row first (one poll per session: a repeat poll
    # long-blocks and trips Django's async-Redis flake constantly).
    # Fresh last_event_at: no sweep may reap this run mid-test.
    _seed_live_state(
        seeder, machine_flow["runner_id"], run["id"], agent_health="healthy", stale=False
    )
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    url = f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{opened['session_id']}/poll"
    response = _post_poll(
        machine_client,
        url,
        {
            "ack": [],
            "status": {
                "status": "busy",
                "in_flight_run": run["id"],
                "observed_run_id": run["id"],
                "agent_health": "bogus-label",
            },
        },
    )
    assert response.status_code == 200, response.text
    row = _live_state_row(seeder, machine_flow["runner_id"])
    assert row["agent_health"] == "healthy"
    assert row["agent_last_activity_at"] == datetime(2026, 10, 20, 1, 2, 3, tzinfo=timezone.utc)


def test_web_runner_detail_live_state_keys(
    user_client, seeder, daemon_world, machine_flow, live_states
):
    """Runner detail ``live_state`` carries both keys in Django field order."""
    live_states["live"].append(machine_flow["runner_id"])
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    instant = "'2026-10-20 01:02:03+00:00'::timestamptz"
    seeder.db.execute(
        "INSERT INTO runner_live_state "
        "(runner_id, observed_run_id, last_event_at, last_event_kind, "
        "approvals_pending, usage, llm_model, turn_count, agent_health, "
        "agent_last_activity_at, updated_at) "
        "VALUES (%s, %s, " + instant + ", 'tool', "
        "0, '{}', 'm', 3, 'unhealthy', " + instant + ", now())",
        (machine_flow["runner_id"], run["id"]),
    )
    response = user_client.get(f"{WEB}/{machine_flow['runner_id']}/")
    assert response.status_code == 200, response.text
    live = response.json()["live_state"]
    assert list(live) == EXPECTED_LIVE_STATE_KEYS
    assert live["agent_health"] == "unhealthy"
    # Same renderer as last_event_at: equal instants render byte-equal.
    assert live["agent_last_activity_at"] == live["last_event_at"]
    assert live["agent_last_activity_at"] is not None
