"""Queued runs behind a busy runner reach a runner that joins the pod (PDASHOSS01-419).

A pod had one runner, busy. Runs queued meanwhile must not wait for it
when a second runner joins: the new runner's first poll drains it, and it
takes the unpinned pod queue and — once that is empty — a locality pin
held by the busy runner (the pin is a hint, not a reservation). A
dedicated pin stays with its runner.

Black box: rows seeded straight into Postgres, the joining runner speaks
the daemon wire (session open + first poll), assertions read the rows.
"""

from __future__ import annotations

import pytest

from _harness.db import wait_for_condition

from .conftest import DAEMON

pytestmark = pytest.mark.contract


def _busy_runner(seeder, daemon_world):
    """The pod's first runner: online, fresh heartbeat, serving a run."""
    runner = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}xbusy",
    )
    seeder.db.execute(
        "UPDATE runner SET status = 'busy', last_heartbeat_at = now() WHERE id = %s",
        (runner["id"],),
    )
    seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        runner["id"],
        status="running",
    )
    return runner["id"]


def _queued(seeder, daemon_world, busy_id, *, pinned_runner_id, pin_kind, parent_id=None):
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        busy_id,
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET runner_id = NULL, pinned_runner_id = %s, pin_kind = %s, "
        "parent_run_id = %s WHERE id = %s",
        (pinned_runner_id, pin_kind, parent_id, run["id"]),
    )
    return run["id"]


def _row(seeder, run_id):
    return seeder.db.fetchone(
        "SELECT status, runner_id, pinned_runner_id, thread_id FROM agent_run WHERE id = %s",
        (run_id,),
    )


def _join(machine_client, runner_id):
    """The new runner's daemon: open a session, then the first poll."""
    opened = machine_client.post(f"{DAEMON}/runners/{runner_id}/sessions/", json={})
    assert opened.status_code == 201, opened.text
    url = f"{DAEMON}/runners/{runner_id}/sessions/{opened.json()['session_id']}/poll"
    response = machine_client.post(url, json={"ack": [], "status": {"status": "idle"}})
    for _ in range(2):  # Django dev-server async-Redis flake (see test_daemon_sessions)
        if response.status_code != 500:
            break
        response = machine_client.post(url, json={"ack": [], "status": {"status": "idle"}})
    assert response.status_code == 200, response.text


def _assigned_to(seeder, run_id, runner_id):
    def check():
        row = _row(seeder, run_id)
        return row if row["status"] == "assigned" and str(row["runner_id"]) == runner_id else None

    return wait_for_condition(check, timeout=20, what=f"run {run_id} assigned to {runner_id}")


def test_never_run_queue_goes_to_joining_runner(seeder, daemon_world, machine_flow, machine_client):
    busy = _busy_runner(seeder, daemon_world)
    first = _queued(seeder, daemon_world, busy, pinned_runner_id=None, pin_kind="locality")
    second = _queued(seeder, daemon_world, busy, pinned_runner_id=None, pin_kind="locality")
    _join(machine_client, machine_flow["runner_id"])
    _assigned_to(seeder, first, machine_flow["runner_id"])
    # One run per runner: the second waits for the next free runner.
    assert _row(seeder, second)["status"] == "queued"


def test_locality_pin_on_busy_runner_goes_to_joining_runner(
    seeder, daemon_world, machine_flow, machine_client
):
    busy = _busy_runner(seeder, daemon_world)
    parent = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        busy,
        status="completed",
    )["id"]
    seeder.db.execute("UPDATE agent_run SET thread_id = 'thread-on-busy' WHERE id = %s", (parent,))
    run = _queued(seeder, daemon_world, busy, pinned_runner_id=busy, pin_kind="locality", parent_id=parent)
    _join(machine_client, machine_flow["runner_id"])
    row = _assigned_to(seeder, run, machine_flow["runner_id"])
    assert row["pinned_runner_id"] is None
    assert _row(seeder, parent)["thread_id"] == ""


@pytest.mark.parametrize(
    ("status", "heartbeat"),
    [("busy", "now() - interval '10 minutes'"), ("offline", "now()")],
)
def test_locality_pin_on_silent_runner_is_not_borrowed(
    seeder, daemon_world, machine_flow, machine_client, status, heartbeat
):
    # A silent runner's stale running row is no proof it is working: its
    # locality pins wait for the offline pin-release sweep and its grace.
    busy = _busy_runner(seeder, daemon_world)
    seeder.db.execute(
        f"UPDATE runner SET status = %s, last_heartbeat_at = {heartbeat} WHERE id = %s",
        (status, busy),
    )
    run = _queued(seeder, daemon_world, busy, pinned_runner_id=busy, pin_kind="locality")
    _join(machine_client, machine_flow["runner_id"])
    row = _row(seeder, run)
    assert row["status"] == "queued"
    assert str(row["pinned_runner_id"]) == busy
    assert row["runner_id"] is None


def test_dedicated_pin_on_busy_runner_stays(seeder, daemon_world, machine_flow, machine_client):
    busy = _busy_runner(seeder, daemon_world)
    run = _queued(seeder, daemon_world, busy, pinned_runner_id=busy, pin_kind="dedicated")
    _join(machine_client, machine_flow["runner_id"])
    row = _row(seeder, run)
    assert row["status"] == "queued"
    assert str(row["pinned_runner_id"]) == busy
    assert row["runner_id"] is None


def test_borrowed_pin_beats_dedicated_after_a_dedicated_run(
    seeder, daemon_world, machine_flow, machine_client
):
    # §10.2 alternation: after a dedicated run the runner owes shared work,
    # and a borrowed locality pin is shared work, so it is served before the
    # runner's next dedicated pin even though that one was queued first.
    joining = machine_flow["runner_id"]
    busy = _busy_runner(seeder, daemon_world)
    served = _queued(seeder, daemon_world, busy, pinned_runner_id=joining, pin_kind="dedicated")
    seeder.db.execute(
        "UPDATE agent_run SET status = 'completed', runner_id = %s, assigned_at = now() WHERE id = %s",
        (joining, served),
    )
    dedicated = _queued(seeder, daemon_world, busy, pinned_runner_id=joining, pin_kind="dedicated")
    borrowable = _queued(seeder, daemon_world, busy, pinned_runner_id=busy, pin_kind="locality")
    _join(machine_client, joining)
    _assigned_to(seeder, borrowable, joining)
    assert _row(seeder, dedicated)["status"] == "queued"
