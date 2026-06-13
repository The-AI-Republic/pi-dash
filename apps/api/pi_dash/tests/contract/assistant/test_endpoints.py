# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from rest_framework.test import APIClient

from pi_dash.assistant.models import AssistantThread
from pi_dash.tests.contract.assistant.conftest import configure_llm

pytestmark = pytest.mark.django_db


def client_for(user):
    c = APIClient()
    c.force_authenticate(user=user)
    return c


def base(ws):
    return f"/api/workspaces/{ws.slug}/assistant"


# --- threads ---

def test_member_can_list_and_create_threads(world):
    c = client_for(world.member)
    assert c.get(f"{base(world.ws)}/threads/").status_code == 200
    res = c.post(f"{base(world.ws)}/threads/", {"title": "Hi"}, format="json")
    assert res.status_code == 201
    assert res.data["title"] == "Hi"


def test_guest_blocked_from_assistant(world):
    c = client_for(world.guest)
    res = c.get(f"{base(world.ws)}/threads/")
    assert res.status_code == 403
    assert res.data["error"] == "role_not_allowed"


def test_non_member_cannot_access(world):
    c = client_for(world.other_user)
    assert c.get(f"{base(world.ws)}/threads/").status_code == 403


# --- messages ---

def test_message_requires_llm_config(world, fernet_key):
    c = client_for(world.member)
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.member)
    res = c.post(f"{base(world.ws)}/threads/{thread.id}/messages/", {"content": "hello"}, format="json")
    assert res.status_code == 422
    assert res.data["error"] == "llm_config_missing"


def test_message_creates_turn_and_blocks_concurrent(world, fernet_key, mocker, django_capture_on_commit_callbacks):
    delay = mocker.patch("pi_dash.assistant.views.messages.run_assistant_turn.delay")
    configure_llm(world.member)
    c = client_for(world.member)
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.member)

    with django_capture_on_commit_callbacks(execute=True):
        res = c.post(f"{base(world.ws)}/threads/{thread.id}/messages/", {"content": "do a thing"}, format="json")
    assert res.status_code == 202
    assert res.data["turn"]["status"] == "queued"
    assert res.data["message"]["role"] == "user"
    assert res.data["message"]["content"] == "do a thing"

    thread.refresh_from_db()
    assert thread.active_turn_id is not None
    assert thread.title == "do a thing"  # auto-title from first message

    # second concurrent post is rejected
    res2 = c.post(f"{base(world.ws)}/threads/{thread.id}/messages/", {"content": "again"}, format="json")
    assert res2.status_code == 409
    assert res2.data["error"] == "turn_active"

    delay.assert_called_once()


def test_message_lists_in_envelope_shape(world, fernet_key, mocker):
    mocker.patch("pi_dash.assistant.views.messages.run_assistant_turn.delay")
    configure_llm(world.member)
    c = client_for(world.member)
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.member)
    c.post(f"{base(world.ws)}/threads/{thread.id}/messages/", {"content": "hi"}, format="json")

    res = c.get(f"{base(world.ws)}/threads/{thread.id}/messages/")
    assert res.status_code == 200
    assert res.data[0]["role"] == "user"
    assert "content" in res.data[0]


def test_message_list_paginates_and_tolerates_bad_params(world, fernet_key):
    from pi_dash.assistant.models import MessageKind
    from pi_dash.assistant.runtime import events

    c = client_for(world.member)
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.member)
    for i in range(3):
        events.create_message(thread, MessageKind.USER, display_content=f"m{i}")
    url = f"{base(world.ws)}/threads/{thread.id}/messages/"

    # after/limit page through the transcript by seq
    res = c.get(url, {"after": 1, "limit": 1})
    assert res.status_code == 200
    assert [m["seq"] for m in res.data] == [2]

    # non-numeric params fall back to defaults instead of a 500
    res = c.get(url, {"after": "abc", "limit": "abc"})
    assert res.status_code == 200
    assert len(res.data) == 3


def test_cannot_access_other_users_thread(world, fernet_key):
    other_thread = AssistantThread.objects.create(workspace=world.ws, user=world.admin)
    c = client_for(world.member)
    res = c.get(f"{base(world.ws)}/threads/{other_thread.id}/messages/")
    assert res.status_code == 404


# --- llm config ---

def test_llm_config_lifecycle(world, fernet_key):
    c = client_for(world.member)
    # unset -> 200 with has_api_key False
    res = c.get("/api/users/me/llm-config/")
    assert res.status_code == 200
    assert res.data["has_api_key"] is False

    # set
    res = c.put(
        "/api/users/me/llm-config/",
        {"provider_kind": "openai_compatible", "base_url": "https://api.example.com/v1", "model_name": "m", "api_key": "sk-12345678"},
        format="json",
    )
    assert res.status_code == 200
    assert res.data["has_api_key"] is True

    # the key is never returned
    assert "api_key" not in res.data

    # delete
    assert c.delete("/api/users/me/llm-config/").status_code == 204
    assert c.get("/api/users/me/llm-config/").data["has_api_key"] is False
