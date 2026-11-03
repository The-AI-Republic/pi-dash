"""Pin wait budget: over-budget pins on busy runners (PDASHOSS01-272).

A follow-up run is pinned to the runner that served the issue's previous
run. When that runner is online, heartbeating and busy with a long run,
the queued follow-up used to wait the whole run out while other runners
in the pod idled. Past the pod's pin wait budget the matcher now hands
the run to an idle runner, clearing the pin and the parent's stale
``thread_id`` exactly as the release-pin hatches do.

Driven through the daemon surface: the idle runner opens a session and
polls with ``status: idle``, whose bookkeeping drains the runner. The
verdict is the run row afterwards.
"""

from __future__ import annotations

import time

import pytest

from _harness.db import wait_for_condition

from .conftest import DAEMON
from .test_daemon_sessions import _open_runner_session, _post_poll

pytestmark = pytest.mark.contract

TIMEOUT = 30.0


def _busy_peer(seeder, daemon_world):
    """A second runner in the pod: online, heartbeating, mid-way through a run."""
    peer = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}xbusy",
    )
    seeder.db.execute(
        "UPDATE runner SET status = 'online', last_heartbeat_at = now() WHERE id = %s",
        (peer["id"],),
    )
    busy_run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        peer["id"],
        status="running",
    )
    seeder.db.execute(
        "UPDATE agent_run SET thread_id = 'thread_abc123', started_at = now() WHERE id = %s",
        (busy_run["id"],),
    )
    return peer, busy_run


def _pinned_queued(seeder, daemon_world, pinned_runner_id, parent_run_id, age_secs):
    """A QUEUED run pinned to ``pinned_runner_id``, created ``age_secs`` ago."""
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        pinned_runner_id,
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET runner_id = NULL, pinned_runner_id = %s, parent_run_id = %s, "
        "created_at = now() - make_interval(secs => %s) WHERE id = %s",
        (pinned_runner_id, parent_run_id, age_secs, run["id"]),
    )
    return run


def _run_row(seeder, run_id):
    return seeder.db.fetchone(
        "SELECT status, runner_id, pinned_runner_id FROM agent_run WHERE id = %s", (run_id,)
    )


def _poll_idle(machine_client, machine_flow, seeder, peer_id):
    """Open a session for the idle flow runner and poll ``idle`` once.

    The busy peer's heartbeat is refreshed first: past ``HEARTBEAT_GRACE``
    (90s) it would read as dead, turning the case into a dead-pin one.
    """
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    seeder.db.execute(
        "UPDATE runner SET last_heartbeat_at = now() WHERE id = %s", (peer_id,)
    )
    url = f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{opened['session_id']}/poll"
    response = _post_poll(machine_client, url, {"ack": [], "status": {"status": "idle"}})
    assert response.status_code == 200, response.text


def test_overbudget_pin_on_busy_runner_goes_to_idle_runner(
    machine_client, machine_flow, daemon_world, seeder
):
    peer, busy_run = _busy_peer(seeder, daemon_world)
    run = _pinned_queued(seeder, daemon_world, peer["id"], busy_run["id"], age_secs=3600)

    _poll_idle(machine_client, machine_flow, seeder, peer["id"])

    def taken_over():
        row = _run_row(seeder, run["id"])
        if row["status"] == "assigned" and str(row["runner_id"]) == machine_flow["runner_id"]:
            return row
        return None

    row = wait_for_condition(taken_over, timeout=TIMEOUT, what="over-budget pin taken over")
    assert row["pinned_runner_id"] is None
    # Same DB state as the release-pin hatches: the parent's stale resume
    # handle is dropped too.
    assert (
        seeder.db.fetchval("SELECT thread_id FROM agent_run WHERE id = %s", (busy_run["id"],))
        == ""
    )


def test_within_budget_pin_is_kept(machine_client, machine_flow, daemon_world, seeder):
    peer, busy_run = _busy_peer(seeder, daemon_world)
    run = _pinned_queued(seeder, daemon_world, peer["id"], busy_run["id"], age_secs=60)

    _poll_idle(machine_client, machine_flow, seeder, peer["id"])
    time.sleep(2)

    row = _run_row(seeder, run["id"])
    assert row["status"] == "queued"
    assert str(row["pinned_runner_id"]) == peer["id"]
    assert (
        seeder.db.fetchval("SELECT thread_id FROM agent_run WHERE id = %s", (busy_run["id"],))
        == "thread_abc123"
    )


def test_pod_budget_override_of_zero_disables_release(
    machine_client, machine_flow, daemon_world, seeder
):
    seeder.db.execute(
        "UPDATE pod SET pin_wait_budget_secs = 0 WHERE id = %s", (daemon_world["pod"]["id"],)
    )
    peer, busy_run = _busy_peer(seeder, daemon_world)
    run = _pinned_queued(seeder, daemon_world, peer["id"], busy_run["id"], age_secs=86400)

    _poll_idle(machine_client, machine_flow, seeder, peer["id"])
    time.sleep(2)

    row = _run_row(seeder, run["id"])
    assert row["status"] == "queued"
    assert str(row["pinned_runner_id"]) == peer["id"]


def test_pod_budget_override_shortens_the_wait(
    machine_client, machine_flow, daemon_world, seeder
):
    seeder.db.execute(
        "UPDATE pod SET pin_wait_budget_secs = 30 WHERE id = %s", (daemon_world["pod"]["id"],)
    )
    peer, busy_run = _busy_peer(seeder, daemon_world)
    run = _pinned_queued(seeder, daemon_world, peer["id"], busy_run["id"], age_secs=120)

    _poll_idle(machine_client, machine_flow, seeder, peer["id"])

    def taken_over():
        row = _run_row(seeder, run["id"])
        return row if row["status"] == "assigned" else None

    row = wait_for_condition(taken_over, timeout=TIMEOUT, what="short-budget pin taken over")
    assert str(row["runner_id"]) == machine_flow["runner_id"]
    assert row["pinned_runner_id"] is None
