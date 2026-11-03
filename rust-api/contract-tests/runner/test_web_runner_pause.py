"""Operator pause / resume of a runner (``POST /api/runners/<id>/pause|resume/``).

Port of Django's ``pi_dash/tests/unit/runner/test_runner_pause.py``
(``09738d222``): a paused runner keeps its connectivity status and
heartbeat but takes no new work anywhere — matcher drains skip it,
pins to it are released, chat to it is refused with ``runner_paused``,
the executor picker reports the paused reasons, and metrics exclude
it from the online gauge while counting it on the paused gauge.
"""

from __future__ import annotations

import pytest

from _harness.web import web_post

from .conftest import DAEMON

WEB = "/api/runners"

pytestmark = pytest.mark.contract


def _open_session(machine_client, runner_id):
    """A live daemon session, so assign frames deliver (``enqueue_for_runner``
    raises ``RunnerOfflineError`` for ``assign`` without one)."""
    response = machine_client.post(f"{DAEMON}/runners/{runner_id}/sessions/", json={})
    assert response.status_code == 201, response.text


def _go_online(seeder, runner_id):
    seeder.db.execute(
        "UPDATE runner SET status = 'online', last_heartbeat_at = now() WHERE id = %s",
        (runner_id,),
    )


def _pause_row(seeder, runner_id):
    seeder.db.execute(
        "UPDATE runner SET paused_at = now() WHERE id = %s", (runner_id,)
    )


def _run_row(seeder, run_id):
    return seeder.db.fetchone(
        "SELECT status, runner_id, pinned_runner_id FROM agent_run WHERE id = %s",
        (run_id,),
    )


def _mk_run(seeder, daemon_world, *, runner_id, status="queued", pinned_runner_id=None):
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        runner_id,
        status=status,
    )
    if status == "queued":
        seeder.db.execute(
            "UPDATE agent_run SET runner_id = NULL, pinned_runner_id = %s WHERE id = %s",
            (pinned_runner_id, run["id"]),
        )
    return run


def test_pause_marks_runner_paused_without_touching_status(
    user_client, daemon_world, machine_flow, seeder
):
    runner_id = machine_flow["runner_id"]
    _go_online(seeder, runner_id)
    response = web_post(user_client, f"{WEB}/{runner_id}/pause/", {})
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["paused_at"] is not None
    assert body["paused_by"] == daemon_world["owner"]["id"]
    assert body["status"] == "online"
    assert body["in_flight_runs"] == []
    row = seeder.db.fetchone(
        "SELECT paused_at, paused_by_id, status FROM runner WHERE id = %s", (runner_id,)
    )
    assert row["paused_at"] is not None
    assert str(row["paused_by_id"]) == daemon_world["owner"]["id"]
    assert row["status"] == "online"


def test_pause_is_idempotent(user_client, machine_flow, seeder):
    runner_id = machine_flow["runner_id"]
    _go_online(seeder, runner_id)
    first = web_post(user_client, f"{WEB}/{runner_id}/pause/", {})
    assert first.status_code == 200, first.text
    second = web_post(user_client, f"{WEB}/{runner_id}/pause/", {})
    assert second.status_code == 200, second.text
    assert second.json()["paused_at"] == first.json()["paused_at"]
    assert second.json()["paused_by"] == first.json()["paused_by"]


def test_pause_while_busy_reports_in_flight_runs(
    user_client, daemon_world, machine_flow, seeder
):
    runner_id = machine_flow["runner_id"]
    _go_online(seeder, runner_id)
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        runner_id,
        status="running",
    )
    response = web_post(user_client, f"{WEB}/{runner_id}/pause/", {})
    assert response.status_code == 200, response.text
    assert response.json()["in_flight_runs"] == [{"id": run["id"], "status": "running"}]
    assert _run_row(seeder, run["id"])["status"] == "running"


@pytest.mark.parametrize("endpoint", ["pause", "resume"])
@pytest.mark.parametrize(
    "revoked",
    [
        {"status": "revoked", "revoked_at": "now"},
        {"status": "revoked"},
        {"revoked_at": "now"},
    ],
    ids=["full", "status-only", "stamp-only"],
)
def test_pause_and_resume_reject_revoked_runner(
    user_client, machine_flow, seeder, endpoint, revoked
):
    runner_id = machine_flow["runner_id"]
    sets = ", ".join(
        f"{col} = now()" if val == "now" else f"{col} = %s" for col, val in revoked.items()
    )
    params = [v for v in revoked.values() if v != "now"] + [runner_id]
    seeder.db.execute(f"UPDATE runner SET {sets} WHERE id = %s", tuple(params))
    if endpoint == "resume":
        _pause_row(seeder, runner_id)
    response = web_post(user_client, f"{WEB}/{runner_id}/{endpoint}/", {})
    assert response.status_code == 409, response.text
    assert response.json()["code"] == "runner_revoked"


def test_pause_requires_manage_permission(user_client, daemon_world, seeder):
    other = seeder.create_user()
    runner = seeder.enroll_runner(
        other["id"], daemon_world["workspace"]["id"], daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}pause-x",
    )
    response = web_post(user_client, f"{WEB}/{runner['id']}/pause/", {})
    # Private runners are invisible to non-owners, so this reads as 404.
    assert response.status_code == 404, response.text
    row = seeder.db.fetchone("SELECT paused_at FROM runner WHERE id = %s", (runner["id"],))
    assert row["paused_at"] is None


def test_pause_unknown_runner_is_404(user_client):
    response = web_post(
        user_client, f"{WEB}/00000000-0000-0000-0000-000000000000/pause/", {}
    )
    assert response.status_code == 404
    assert response.json() == {"error": "not found"}


def test_pause_releases_pinned_runs_to_another_runner(
    user_client, daemon_world, machine_flow, machine_client, seeder
):
    owner_id = daemon_world["owner"]["id"]
    ws_id = daemon_world["workspace"]["id"]
    pod_id = daemon_world["pod"]["id"]
    other_id = machine_flow["runner_id"]
    _open_session(machine_client, other_id)
    first = seeder.enroll_runner(owner_id, ws_id, pod_id, f"apd_en_{seeder.tag}pause-r1")
    first_id = first["id"]
    _go_online(seeder, first_id)
    _go_online(seeder, other_id)
    # Busy, so the pinned follow-up waits rather than being already assigned.
    _mk_run(seeder, daemon_world, runner_id=first_id, status="running")
    pinned = _mk_run(seeder, daemon_world, runner_id=first_id, pinned_runner_id=first_id)

    response = web_post(user_client, f"{WEB}/{first_id}/pause/", {})
    assert response.status_code == 200, response.text
    row = _run_row(seeder, pinned["id"])
    assert row["pinned_runner_id"] is None
    assert str(row["runner_id"]) == other_id
    assert row["status"] == "assigned"


def test_pause_release_drains_only_to_idle_runners(
    user_client, daemon_world, machine_flow, seeder
):
    """Both runners paused: the release unpins but nothing is assigned."""
    owner_id = daemon_world["owner"]["id"]
    ws_id = daemon_world["workspace"]["id"]
    pod_id = daemon_world["pod"]["id"]
    first_id = machine_flow["runner_id"]
    _go_online(seeder, first_id)
    other = seeder.enroll_runner(owner_id, ws_id, pod_id, f"apd_en_{seeder.tag}pause-r2")
    _go_online(seeder, other["id"])
    _pause_row(seeder, first_id)
    stuck = _mk_run(seeder, daemon_world, runner_id=first_id, pinned_runner_id=first_id)
    freed = _mk_run(seeder, daemon_world, runner_id=other["id"], pinned_runner_id=other["id"])

    response = web_post(user_client, f"{WEB}/{other['id']}/pause/", {})
    assert response.status_code == 200, response.text
    stuck_row = _run_row(seeder, stuck["id"])
    assert stuck_row["status"] == "queued"
    assert stuck_row["runner_id"] is None
    assert str(stuck_row["pinned_runner_id"]) == first_id
    freed_row = _run_row(seeder, freed["id"])
    assert freed_row["status"] == "queued"
    assert freed_row["runner_id"] is None
    assert freed_row["pinned_runner_id"] is None


def test_revoke_drain_routes_around_sql_paused_runner(
    user_client, daemon_world, machine_flow, machine_client, seeder
):
    """A runner paused out-of-band (DB stamp, as after a Django pause)
    takes nothing from a pod drain: its pinned run waits, other runs go
    to the idle runner."""
    owner_id = daemon_world["owner"]["id"]
    ws_id = daemon_world["workspace"]["id"]
    pod_id = daemon_world["pod"]["id"]
    idle_id = machine_flow["runner_id"]
    _open_session(machine_client, idle_id)
    paused = seeder.enroll_runner(owner_id, ws_id, pod_id, f"apd_en_{seeder.tag}pause-p1")
    paused_id = paused["id"]
    _go_online(seeder, paused_id)
    _go_online(seeder, idle_id)
    doomed = seeder.enroll_runner(owner_id, ws_id, pod_id, f"apd_en_{seeder.tag}pause-doom")
    _go_online(seeder, doomed["id"])
    _pause_row(seeder, paused_id)
    waiting = _mk_run(seeder, daemon_world, runner_id=paused_id, pinned_runner_id=paused_id)
    drifting = _mk_run(seeder, daemon_world, runner_id=doomed["id"], pinned_runner_id=doomed["id"])

    revoked = web_post(user_client, f"{WEB}/{doomed['id']}/revoke/", {})
    assert revoked.status_code == 200, revoked.text
    waiting_row = _run_row(seeder, waiting["id"])
    assert waiting_row["status"] == "queued"
    assert waiting_row["runner_id"] is None
    assert str(waiting_row["pinned_runner_id"]) == paused_id
    drifting_row = _run_row(seeder, drifting["id"])
    assert drifting_row["status"] == "assigned"
    assert str(drifting_row["runner_id"]) == idle_id


def test_resume_clears_pause_and_drains_queued_work(
    user_client, machine_flow, machine_client, seeder, daemon_world
):
    runner_id = machine_flow["runner_id"]
    _open_session(machine_client, runner_id)
    _go_online(seeder, runner_id)
    _pause_row(seeder, runner_id)
    run = _mk_run(seeder, daemon_world, runner_id=runner_id, pinned_runner_id=runner_id)

    response = web_post(user_client, f"{WEB}/{runner_id}/resume/", {})
    assert response.status_code == 200, response.text
    assert response.json()["paused_at"] is None
    assert response.json()["paused_by"] is None
    row = seeder.db.fetchone(
        "SELECT paused_at, paused_by_id FROM runner WHERE id = %s", (runner_id,)
    )
    assert row["paused_at"] is None
    assert row["paused_by_id"] is None
    assigned = _run_row(seeder, run["id"])
    assert assigned["status"] == "assigned"
    assert str(assigned["runner_id"]) == runner_id


def test_resume_unpaused_runner_does_not_drain(
    user_client, machine_flow, seeder, daemon_world
):
    runner_id = machine_flow["runner_id"]
    _go_online(seeder, runner_id)
    run = _mk_run(seeder, daemon_world, runner_id=runner_id)
    response = web_post(user_client, f"{WEB}/{runner_id}/resume/", {})
    assert response.status_code == 200, response.text
    assert _run_row(seeder, run["id"])["status"] == "queued"


def test_executor_options_report_paused_local_runner(
    user_client, daemon_world, machine_flow, seeder
):
    ws = daemon_world["workspace"]
    project = daemon_world["project"]
    seeder.create_project_member(
        ws["id"], project["id"], daemon_world["owner"]["id"]
    )
    _go_online(seeder, machine_flow["runner_id"])
    _pause_row(seeder, machine_flow["runner_id"])
    response = user_client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/")
    assert response.status_code == 200, response.text
    options = {row["kind"]: row for row in response.json()["agent_executor_options"]}
    assert options["local_runner"]["available"] is False
    assert options["local_runner"]["reason_code"] == "local_runner_paused"
