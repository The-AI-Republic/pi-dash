"""Daemon run lifecycle: ``runs/<id>/accept|queued|started|events|approvals|...``.

Every endpoint is runner-scoped (``_resolve`` → 404 unknown run, 403 foreign
run) and idempotency-aware (``Idempotency-Key`` → ``{ok: true, duplicate}``).
Terminal transitions each get a fresh seeded run — one run exercises exactly
one terminal path.
"""

from __future__ import annotations

import uuid

import pytest

from _harness.http import bearer_client

from .conftest import DAEMON

pytestmark = pytest.mark.contract


def _idem() -> dict:
    return {"Idempotency-Key": uuid.uuid4().hex}


def test_daemon_run_accept_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    assert response.status_code == 200, response.text
    assert response.headers["allow"] == "POST, OPTIONS"
    assert response.json() == {"ok": True}
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "running"
    )


def test_daemon_run_endpoints_dedupe(machine_client, daemon_run):
    key = _idem()
    first = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={}, headers=key
    )
    assert first.json() == {"ok": True}
    assert first.headers["allow"] == "POST, OPTIONS"
    second = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={}, headers=key
    )
    assert second.json() == {"ok": True, "duplicate": True}
    assert second.headers["allow"] == "POST, OPTIONS"


def test_daemon_run_queued_is_ack_only(machine_client, daemon_run, seeder):
    response = machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/queued/", json={})
    assert response.json() == {"ok": True, "ignored": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    # The retired worktree queue performs no transition: still assigned.
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "assigned"
    )


def test_daemon_run_started_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/started/",
        json={"thread_id": "thr-1", "model": "test-model"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    row = seeder.db.fetchone(
        "SELECT status, thread_id, llm_model FROM agent_run WHERE id = %s", (daemon_run["id"],)
    )
    assert row["status"] == "running"
    assert row["thread_id"] == "thr-1"
    assert row["llm_model"] == "test-model"


def test_daemon_run_events_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/events/",
        json={"events": [{"seq": 1, "kind": "log", "payload": {"text": "hi"}}]},
        headers=_idem(),
    )
    assert response.json() == {"ok": True, "accepted": 1}
    assert response.headers["allow"] == "POST, OPTIONS"
    assert (
        seeder.db.fetchval(
            "SELECT COUNT(*) FROM agent_run_event WHERE agent_run_id = %s", (daemon_run["id"],)
        )
        == 1
    )


def test_daemon_run_approvals_shape(machine_client, daemon_run, seeder):
    approval_id = str(uuid.uuid4())
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/approvals/",
        json={"approval_id": approval_id, "kind": "command_execution", "reason": "rm -rf /"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    row = seeder.db.fetchone(
        "SELECT status FROM agent_run_approval WHERE id = %s", (approval_id,)
    )
    assert row["status"] == "pending"
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "awaiting_approval"
    )


def test_daemon_run_awaiting_reauth_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/awaiting-reauth/", json={}, headers=_idem()
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "awaiting_reauth"
    )


def test_daemon_run_complete_shape(machine_client, daemon_run, seeder):
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/complete/",
        json={"done_payload": {"summary": "done"}},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "completed"
    )


def test_daemon_run_pause_shape(machine_client, daemon_run):
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/pause/",
        json={"payload": {"question": "proceed?"}},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"


def test_daemon_run_fail_shape(machine_client, daemon_run, seeder):
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/fail/",
        json={"detail": "boom"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "failed"
    )


def test_daemon_run_fail_refusal_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/fail/",
        json={"reason": "refusal", "category": "cyber", "detail": "declined"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True, "refused": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "refused"
    )


def test_daemon_run_cancelled_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/cancelled/", json={}, headers=_idem()
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "cancelled"
    )


def test_daemon_run_resumed_shape(machine_client, daemon_run, seeder):
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/resumed/", json={}, headers=_idem()
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "running"
    )


def test_daemon_run_stream_upgrade_shape(machine_client, daemon_run):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/stream/upgrade/", json={"stream": "events"}
    )
    assert response.status_code == 200, response.text
    assert response.headers["allow"] == "POST, OPTIONS"
    body = response.json()
    assert body["ticket"]
    assert body["expires_in_secs"] == 60


def test_daemon_run_stream_upgrade_rejects_stream(machine_client, daemon_run):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/stream/upgrade/", json={"stream": "video"}
    )
    assert response.status_code == 400
    assert response.headers["allow"] == "POST, OPTIONS"
    assert response.json() == {"error": "invalid_stream"}


def test_daemon_run_not_found(machine_client):
    response = machine_client.post(f"{DAEMON}/runs/{uuid.uuid4()}/accept/", json={})
    assert response.status_code == 404
    assert response.headers["allow"] == "POST, OPTIONS"
    assert response.json() == {"error": "run_not_found"}


def test_daemon_run_rejects_foreign_runner(settings, seeder, daemon_world, daemon_run):
    """Tenant isolation: a run owned by another runner (other workspace) is 403."""
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    other_project = seeder.create_project(other_ws["id"])
    other_pod = seeder.create_pod(other_ws["id"], other_project["id"], is_default=True)
    other_runner = seeder.enroll_runner(
        other_owner["id"], other_ws["id"], other_pod["id"], f"apd_en_{seeder.tag}foreign1"
    )
    # Enroll the foreign runner to get a real access token for it.
    import httpx

    with httpx.Client(base_url=settings.base_url, timeout=30.0) as anon:
        enroll = anon.post(
            f"{DAEMON}/runners/enroll/",
            json={"enrollment_token": other_runner["enrollment_token"], "host_label": "foreign-h"},
        )
        assert enroll.status_code == 201, enroll.text
        foreign_token = enroll.json()["access_token"]
    with bearer_client(settings.base_url, foreign_token) as foreign_client:
        response = foreign_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    assert response.status_code == 403
    assert response.headers["allow"] == "POST, OPTIONS"
    assert response.json() == {"error": "run_not_owned_by_runner"}
    # Teardown rows the foreign enroll minted.
    seeder.db.execute(
        "DELETE FROM machine_token WHERE user_id = %s AND host_label = 'foreign-h'",
        (other_owner["id"],),
    )
    seeder.db.execute(
        "UPDATE runner SET dev_machine_id = NULL WHERE id = %s", (other_runner["id"],)
    )
    seeder.db.execute(
        "DELETE FROM dev_machine WHERE owner_id = %s AND host_label = 'foreign-h'",
        (other_owner["id"],),
    )


def test_daemon_run_requires_auth(anon_client, daemon_run):
    # No bearer token: auth returns None and the run resolves as not-owned.
    response = anon_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    assert response.status_code == 403
    assert response.headers["allow"] == "POST, OPTIONS"
    assert response.json() == {"error": "run_not_owned_by_runner"}


@pytest.fixture()
def comment_world(seeder, daemon_world, machine_flow):
    """An issue plus an assigned daemon run attached to it (PIDASHCONV-1483).

    Server-created rows (comment, description) are removed afterwards — the
    seed tracker only knows seeded rows.
    """
    state = seeder.create_state(
        daemon_world["workspace"]["id"], daemon_world["project"]["id"]
    )
    issue = seeder.create_issue(
        daemon_world["workspace"]["id"],
        daemon_world["project"]["id"],
        state["id"],
        name="lifecycle comment issue",
    )
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="assigned",
    )
    seeder.db.execute(
        "UPDATE agent_run SET work_item_id = %s WHERE id = %s",
        (issue["id"], run["id"]),
    )
    world = {
        "issue_id": issue["id"],
        "run_id": run["id"],
        "owner_id": daemon_world["owner"]["id"],
    }
    yield world
    seeder.db.execute("DELETE FROM run_message_dedupe WHERE run_id = %s", (run["id"],))
    seeder.db.execute("DELETE FROM agent_run_event WHERE agent_run_id = %s", (run["id"],))
    seeder.db.execute(
        "DELETE FROM agent_run_approval WHERE agent_run_id = %s", (run["id"],)
    )
    descriptions = seeder.db.fetchall(
        "SELECT description_id FROM issue_comments WHERE issue_id = %s",
        (issue["id"],),
    )
    seeder.db.execute("DELETE FROM issue_comments WHERE issue_id = %s", (issue["id"],))
    for row in descriptions:
        if row["description_id"] is not None:
            seeder.db.execute(
                "DELETE FROM descriptions WHERE id = %s", (row["description_id"],)
            )


def _assert_owner_stamped(seeder, comment_world, where, params, body_marker):
    comments = seeder.db.fetchall(
        "SELECT id, created_by_id, updated_by_id, description_id, comment_html"
        f" FROM issue_comments WHERE {where} AND deleted_at IS NULL",
        params,
    )
    assert len(comments) == 1
    comment = comments[0]
    assert body_marker in comment["comment_html"]
    assert str(comment["created_by_id"]) == comment_world["owner_id"]
    assert comment["updated_by_id"] is None
    description = seeder.db.fetchone(
        "SELECT created_by_id, updated_by_id FROM descriptions WHERE id = %s",
        (comment["description_id"],),
    )
    assert description is not None
    assert str(description["created_by_id"]) == comment_world["owner_id"]
    assert description["updated_by_id"] is None


def test_daemon_run_pause_comment_stamped_with_owner(
    machine_client, seeder, comment_world
):
    machine_client.post(f"{DAEMON}/runs/{comment_world['run_id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{comment_world['run_id']}/pause/",
        json={"payload": {"autonomy": {"question_for_human": "Which DB?"}}},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    _assert_owner_stamped(
        seeder,
        comment_world,
        "issue_id = %s",
        (comment_world["issue_id"],),
        "Which DB?",
    )


def test_daemon_run_fail_comment_stamped_with_owner(
    machine_client, seeder, comment_world
):
    machine_client.post(f"{DAEMON}/runs/{comment_world['run_id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{comment_world['run_id']}/fail/",
        json={"detail": "boom"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    assert (
        seeder.db.fetchval(
            "SELECT status FROM agent_run WHERE id = %s", (comment_world["run_id"],)
        )
        == "failed"
    )
    _assert_owner_stamped(
        seeder,
        comment_world,
        "issue_id = %s AND speaker_agent_run_id = %s",
        (comment_world["issue_id"], comment_world["run_id"]),
        "boom",
    )


# -- stale-busy release on finalization (PDASHOSS01-231) ------------------

def _set_runner_status(seeder, runner_id: str, status: str) -> None:
    seeder.db.execute("UPDATE runner SET status = %s WHERE id = %s", (status, runner_id))


def _runner_status(seeder, runner_id: str) -> str:
    return seeder.db.fetchval("SELECT status FROM runner WHERE id = %s", (runner_id,))


def test_daemon_run_shutdown_fail_releases_busy_runner(
    machine_client, machine_flow, daemon_run, seeder
):
    # The daemon-shutdown drain fails the in-flight run; the runner row
    # must not stay ``busy`` once nothing occupies it.
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    _set_runner_status(seeder, machine_flow["runner_id"], "busy")
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/fail/",
        json={"detail": "daemon shutdown requested"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "failed"
    )
    assert _runner_status(seeder, machine_flow["runner_id"]) == "online"


def test_daemon_run_complete_releases_busy_runner(
    machine_client, machine_flow, daemon_run, seeder
):
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    _set_runner_status(seeder, machine_flow["runner_id"], "busy")
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/complete/",
        json={"done_payload": {"summary": "done"}},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert _runner_status(seeder, machine_flow["runner_id"]) == "online"


def test_daemon_run_fail_keeps_runner_busy_with_other_active_run(
    machine_client, machine_flow, daemon_run, daemon_world, seeder
):
    seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="running",
    )
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    _set_runner_status(seeder, machine_flow["runner_id"], "busy")
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/fail/",
        json={"detail": "boom"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert _runner_status(seeder, machine_flow["runner_id"]) == "busy"


def test_daemon_run_fail_never_resurrects_offline_runner(
    machine_client, machine_flow, daemon_run, seeder
):
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    _set_runner_status(seeder, machine_flow["runner_id"], "offline")
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/fail/",
        json={"detail": "daemon shutdown requested"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert _runner_status(seeder, machine_flow["runner_id"]) == "offline"
