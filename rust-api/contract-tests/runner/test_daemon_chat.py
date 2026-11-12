"""Daemon chat lifecycle: ``chat/sessions/<sid>/started|events|...|closed``.

Started/events/failed/closed require ``Idempotency-Key`` (400 without);
message-started/complete dedupe on the message id instead. Approvals create
``AgentChatApprovalRequest`` rows. Each terminal path gets a fresh session.
"""

from __future__ import annotations

import uuid

import pytest

from .conftest import DAEMON

pytestmark = pytest.mark.contract


def _idem() -> dict:
    return {"Idempotency-Key": uuid.uuid4().hex}


def _sid(chat_world) -> str:
    return chat_world["session"]["id"]


def test_daemon_chat_started_shape(machine_client, chat_world, seeder):
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{_sid(chat_world)}/started/",
        json={"local_thread_id": "lt-1", "agent_kind": "test"},
        headers=_idem(),
    )
    assert response.status_code == 200, response.text
    assert response.json() == {"ok": True}
    row = seeder.db.fetchone(
        "SELECT local_thread_id, agent_kind FROM agent_chat_session WHERE id = %s",
        (_sid(chat_world),),
    )
    assert row["local_thread_id"] == "lt-1"
    assert row["agent_kind"] == "test"


def test_daemon_chat_started_requires_idempotency_key(machine_client, chat_world):
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{_sid(chat_world)}/started/", json={}
    )
    assert response.status_code == 400
    assert response.json() == {"error": "idempotency_key_required"}


def test_daemon_chat_message_lifecycle(machine_client, chat_world, seeder):
    sid = _sid(chat_world)
    mid = chat_world["message"]["id"]
    machine_client.post(
        f"{DAEMON}/chat/sessions/{sid}/started/", json={}, headers=_idem()
    )
    started = machine_client.post(
        f"{DAEMON}/chat/sessions/{sid}/messages/{mid}/started/",
        json={"turn_id": "turn-1"},
    )
    assert started.json() == {"ok": True}
    # Same start twice dedupes on the message id.
    assert (
        machine_client.post(
            f"{DAEMON}/chat/sessions/{sid}/messages/{mid}/started/",
            json={"turn_id": "turn-1"},
        ).json()
        == {"ok": True, "duplicate": True}
    )
    complete = machine_client.post(
        f"{DAEMON}/chat/sessions/{sid}/messages/{mid}/complete/",
        json={"assistant_message": "done", "status": "completed"},
    )
    assert complete.json() == {"ok": True}
    row = seeder.db.fetchone(
        "SELECT content FROM agent_chat_message WHERE session_id = %s AND role = 'assistant'",
        (sid,),
    )
    assert row is not None and "done" in (row["content"] or "")


def test_daemon_chat_events_shape(machine_client, chat_world):
    sid = _sid(chat_world)
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{sid}/events/",
        json={"kind": "raw", "payload": {"text": "ping"}},
        headers=_idem(),
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["ok"] is True
    assert body["event"]["kind"] == "raw"


def test_daemon_chat_events_requires_idempotency_key(machine_client, chat_world):
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{_sid(chat_world)}/events/",
        json={"kind": "raw", "payload": {}},
    )
    assert response.status_code == 400
    assert response.json() == {"error": "idempotency_key_required"}


def test_daemon_chat_approvals_shape(machine_client, chat_world, seeder):
    sid = _sid(chat_world)
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{sid}/approvals/",
        json={"local_approval_id": "la-1", "kind": "shell", "reason": "run tests"},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["ok"] is True
    assert body["approval"]["local_approval_id"] == "la-1"
    assert (
        seeder.db.fetchval(
            "SELECT COUNT(*) FROM agent_chat_approval WHERE session_id = %s", (sid,)
        )
        == 1
    )


def test_daemon_chat_approvals_requires_local_id(machine_client, chat_world):
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{_sid(chat_world)}/approvals/", json={}
    )
    assert response.status_code == 400
    assert response.json() == {"error": "local_approval_id_required"}


def test_daemon_chat_failed_shape(machine_client, chat_world, seeder):
    sid = _sid(chat_world)
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{sid}/failed/",
        json={"code": "boom", "detail": "agent crashed"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert (
        seeder.db.fetchval("SELECT error FROM agent_chat_session WHERE id = %s", (sid,))
        == "agent crashed"
    )


def test_daemon_chat_closed_shape(machine_client, chat_world, seeder):
    sid = _sid(chat_world)
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{sid}/closed/",
        json={"reason": "user_done"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert (
        seeder.db.fetchval("SELECT status FROM agent_chat_session WHERE id = %s", (sid,))
        == "closed"
    )


def test_daemon_chat_session_not_found(machine_client):
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{uuid.uuid4()}/started/", json={}, headers=_idem()
    )
    assert response.status_code == 404
    assert response.json() == {"error": "chat_session_not_found"}


def test_daemon_chat_rejects_foreign_runner(
    settings, seeder, daemon_world, chat_world, machine_client
):
    """Tenant isolation: a session owned by another runner is 403."""
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    other_project = seeder.create_project(other_ws["id"])
    other_pod = seeder.create_pod(other_ws["id"], other_project["id"], is_default=True)
    other_runner = seeder.enroll_runner(
        other_owner["id"], other_ws["id"], other_pod["id"], f"apd_en_{seeder.tag}chatfrn1"
    )
    foreign_session = seeder.create_chat_session(
        other_ws["id"], other_runner["id"], other_pod["id"], other_owner["id"]
    )
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{foreign_session['id']}/started/",
        json={},
        headers=_idem(),
    )
    assert response.status_code == 403
    assert response.json() == {"error": "chat_session_not_owned_by_runner"}


def test_daemon_chat_requires_auth(anon_client, chat_world):
    # BUG (port it, don't fix it): with no bearer token the chat base
    # dereferences ``request.auth_runner.id`` on None (``_resolve``) — an
    # unhandled AttributeError, so Django answers 500 instead of 401/403.
    # Only the status is pinned (the body is Django's HTML error page).
    response = anon_client.post(
        f"{DAEMON}/chat/sessions/{_sid(chat_world)}/started/", json={}, headers=_idem()
    )
    assert response.status_code == 500
