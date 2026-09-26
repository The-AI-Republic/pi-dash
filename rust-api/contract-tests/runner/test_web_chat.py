"""Web direct-runner chat (``/api/runners/chat/...``).

Session CRUD + warm/message/cancel/close, chat approvals list/decide, and
the SSE event stream (content-type, replay framing, close behaviour). Chat
approvals are seeded black-box through the daemon approval endpoint. The
seeded runner is OFFLINE, so warm/message take the ``409
runner_unavailable`` path unless a test flips the status column online.
"""

from __future__ import annotations

import json
import uuid

import pytest

from _harness.sse import EVENT_STREAM_CONTENT_TYPE, collect_frames
from _harness.web import web_post

WEB = "/api/runners"
DAEMON = "/api/v1/runner"

pytestmark = pytest.mark.contract

SESSION_KEYS = {
    "id", "workspace", "runner", "runner_detail", "created_by", "pod",
    "status", "agent_kind", "local_thread_id", "local_session_id", "cwd",
    "model", "active_turn_id", "active_message_id", "close_requested",
    "last_message_at", "closed_at", "error", "created_at", "updated_at",
}

MESSAGE_KEYS = {
    "id", "session", "role", "content", "content_parts", "status",
    "local_item_id", "local_turn_id", "seq", "created_at", "completed_at",
}

CHAT_APPROVAL_KEYS = {
    "id", "session", "local_approval_id", "kind", "payload", "reason",
    "status", "decision_source", "decided_by", "requested_at", "expires_at",
    "decided_at",
}


def _sid(chat_world) -> str:
    return chat_world["session"]["id"]


def _go_online(seeder, machine_flow):
    seeder.db.execute("UPDATE runner SET status = 'online' WHERE id = %s", (machine_flow["runner_id"],))


def test_web_chat_sessions_list_shape(user_client, daemon_world, chat_world):
    response = user_client.get(
        f"{WEB}/chat/sessions/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert isinstance(body, list) and len(body) == 1
    session = body[0]
    assert SESSION_KEYS <= set(session)
    assert session["id"] == _sid(chat_world)
    assert session["status"] == "open"
    assert session["runner_detail"]["id"] == session["runner"]


def test_web_chat_sessions_list_denies_anonymous(anon_client):
    response = anon_client.get(f"{WEB}/chat/sessions/")
    assert response.status_code == 401
    assert response.json() == {"detail": "Authentication credentials were not provided."}


def test_web_chat_sessions_list_isolates_tenants(user_client, seeder, daemon_world, chat_world):
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    forbidden = user_client.get(f"{WEB}/chat/sessions/", params={"workspace": other_ws["id"]})
    assert forbidden.status_code == 403
    own = user_client.get(
        f"{WEB}/chat/sessions/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert [s["id"] for s in own.json()] == [_sid(chat_world)]


def test_web_chat_session_detail_shape(user_client, chat_world):
    response = user_client.get(f"{WEB}/chat/sessions/{_sid(chat_world)}/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert SESSION_KEYS <= set(body)
    assert body["id"] == _sid(chat_world)


def test_web_chat_session_detail_unknown_is_404(user_client):
    response = user_client.get(f"{WEB}/chat/sessions/00000000-0000-0000-0000-000000000000/")
    assert response.status_code == 404
    assert response.json() == {"error": "not found"}


def test_web_chat_session_create_validates_input(user_client, daemon_world, machine_flow, seeder):
    _go_online(seeder, machine_flow)
    missing = web_post(user_client, f"{WEB}/chat/sessions/", {})
    assert missing.status_code == 400
    assert missing.json() == {"error": "workspace and runner are required"}
    created = web_post(
        user_client,
        f"{WEB}/chat/sessions/",
        {"workspace": daemon_world["workspace"]["id"], "runner": machine_flow["runner_id"]},
    )
    # The seeded session already holds a message, so no empty-open reuse.
    assert created.status_code == 201, created.text
    assert SESSION_KEYS <= set(created.json())


def test_web_chat_session_create_offline_runner_is_409(user_client, daemon_world, machine_flow):
    response = web_post(
        user_client,
        f"{WEB}/chat/sessions/",
        {"workspace": daemon_world["workspace"]["id"], "runner": machine_flow["runner_id"]},
    )
    assert response.status_code == 409
    assert response.json() == {"error": "runner_unavailable"}


def test_web_chat_messages_list_shape(user_client, chat_world):
    response = user_client.get(f"{WEB}/chat/sessions/{_sid(chat_world)}/messages/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert isinstance(body, list) and len(body) == 1
    message = body[0]
    assert MESSAGE_KEYS <= set(message)
    assert message["role"] == "user" and message["content"] == "hello"


def test_web_chat_message_requires_content(user_client, chat_world):
    response = web_post(user_client, f"{WEB}/chat/sessions/{_sid(chat_world)}/messages/", {})
    assert response.status_code == 400
    assert response.json() == {"error": "content is required"}


def test_web_chat_message_offline_runner_is_409(user_client, chat_world):
    response = web_post(
        user_client,
        f"{WEB}/chat/sessions/{_sid(chat_world)}/messages/",
        {"content": "hello?"},
    )
    assert response.status_code == 409
    assert response.json() == {"error": "runner_unavailable"}


def test_web_chat_message_shape_online(user_client, chat_world, machine_flow, seeder):
    _go_online(seeder, machine_flow)
    response = web_post(
        user_client,
        f"{WEB}/chat/sessions/{_sid(chat_world)}/messages/",
        {"content": "online hello"},
    )
    assert response.status_code == 201, response.text
    body = response.json()
    assert MESSAGE_KEYS <= set(body)
    assert body["content"] == "online hello" and body["status"] == "queued"


def test_web_chat_warm_offline_is_409(user_client, chat_world):
    response = web_post(user_client, f"{WEB}/chat/sessions/{_sid(chat_world)}/warm/", {})
    assert response.status_code == 409
    assert response.json() == {"error": "runner_unavailable"}


def test_web_chat_warm_shape_online(user_client, chat_world, machine_flow, seeder):
    _go_online(seeder, machine_flow)
    response = web_post(user_client, f"{WEB}/chat/sessions/{_sid(chat_world)}/warm/", {})
    assert response.status_code == 202, response.text
    assert response.json() == {"ok": True}


def test_web_chat_cancel_noop_shape(user_client, chat_world):
    response = web_post(user_client, f"{WEB}/chat/sessions/{_sid(chat_world)}/cancel/", {})
    assert response.status_code == 200, response.text
    assert response.json() == {"ok": True, "noop": True}


def test_web_chat_close_shape(user_client, chat_world, machine_flow, seeder):
    """Close persists CLOSED, then the offline fan-out answers 500 (Django bug).

    ``send_to_runner`` raises ``RunnerOfflineError`` inside ``on_commit`` —
    after the session row committed — so the 500 carries a persisted close.
    The oracle pins both halves.
    """
    response = web_post(user_client, f"{WEB}/chat/sessions/{_sid(chat_world)}/close/", {})
    assert response.status_code == 500, response.text
    assert (
        seeder.db.fetchval(
            "SELECT status FROM agent_chat_session WHERE id = %s", (_sid(chat_world),)
        )
        == "closed"
    )


def _seed_chat_approval(machine_client, chat_world) -> str:
    local_id = f"la-{uuid.uuid4().hex[:8]}"
    response = machine_client.post(
        f"{DAEMON}/chat/sessions/{_sid(chat_world)}/approvals/",
        json={"local_approval_id": local_id, "kind": "shell", "reason": "run tests"},
    )
    assert response.status_code == 200, response.text
    assert response.json()["approval"]["local_approval_id"] == local_id
    return response.json()["approval"]["id"]


def test_web_chat_approvals_list_shape(user_client, daemon_world, machine_client, chat_world):
    approval_id = _seed_chat_approval(machine_client, chat_world)
    response = user_client.get(
        f"{WEB}/chat/approvals/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert isinstance(body, list) and len(body) == 1
    approval = body[0]
    assert CHAT_APPROVAL_KEYS <= set(approval)
    assert approval["id"] == approval_id
    assert approval["status"] == "pending"


def test_web_chat_approvals_list_denies_anonymous(anon_client):
    response = anon_client.get(f"{WEB}/chat/approvals/")
    assert response.status_code == 401
    assert response.json() == {"detail": "Authentication credentials were not provided."}


def test_web_chat_approval_decide_rejects_bad_decision(user_client, machine_client, chat_world):
    approval_id = _seed_chat_approval(machine_client, chat_world)
    response = web_post(
        user_client, f"{WEB}/chat/approvals/{approval_id}/decide/", {"decision": "maybe"}
    )
    assert response.status_code == 400


def test_web_chat_approval_decide_shape(user_client, machine_client, chat_world, seeder):
    """Decide persists, then the offline fan-out answers 500 (same Django bug)."""
    approval_id = _seed_chat_approval(machine_client, chat_world)
    response = web_post(
        user_client, f"{WEB}/chat/approvals/{approval_id}/decide/", {"decision": "accept"}
    )
    assert response.status_code == 500, response.text
    assert (
        seeder.db.fetchval(
            "SELECT status FROM agent_chat_approval WHERE id = %s", (approval_id,)
        )
        == "accepted"
    )


def test_web_chat_events_replay_shape(user_client, machine_client, chat_world, seeder):
    """SSE replay: content-type, ``chat.event`` framing, then client close."""
    posted = machine_client.post(
        f"{DAEMON}/chat/sessions/{_sid(chat_world)}/events/",
        json={"kind": "note", "payload": {"text": "hi"}},
        headers={"Idempotency-Key": uuid.uuid4().hex},
    )
    assert posted.json()["ok"] is True, posted.text
    assert posted.json()["event"]["kind"] == "note"
    headers, frames = collect_frames(
        user_client, "GET", f"{WEB}/chat/sessions/{_sid(chat_world)}/events/",
        want_frames=1, timeout_s=25.0,
    )
    assert EVENT_STREAM_CONTENT_TYPE in headers.get("content-type", "")
    events = [f for f in frames if f["event"] == "chat.event"]
    assert len(events) == 1
    frame = events[0]
    assert frame["id"] and frame["id"].isdigit()
    data = json.loads("\n".join(frame["data"]))
    assert data["kind"] == "note"


def test_web_chat_events_deny_anonymous(anon_client, chat_world):
    # Plain-function view (not DRF): unauthenticated answers 403 JSON.
    response = anon_client.get(f"{WEB}/chat/sessions/{_sid(chat_world)}/events/")
    assert response.status_code == 403
    assert response.json() == {"error": "authentication required"}


def test_web_chat_events_unknown_is_404(user_client):
    response = user_client.get(
        f"{WEB}/chat/sessions/00000000-0000-0000-0000-000000000000/events/"
    )
    assert response.status_code == 404
    assert response.json() == {"error": "not found"}
