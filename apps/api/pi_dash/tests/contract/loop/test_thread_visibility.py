# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop threads are hidden from the assistant thread list but still
owner-accessible by id; new threads are always chat."""

from __future__ import annotations

import pytest
from rest_framework.test import APIClient

from pi_dash.assistant.models import AssistantThread, ThreadKind

pytestmark = pytest.mark.django_db


def _client(user):
    c = APIClient()
    c.force_authenticate(user=user)
    return c


def test_thread_list_excludes_loop_threads(world):
    AssistantThread.objects.create(workspace=world.ws, user=world.member, kind=ThreadKind.CHAT, title="chat")
    AssistantThread.objects.create(workspace=world.ws, user=world.member, kind=ThreadKind.LOOP, title="loop")
    c = _client(world.member)
    res = c.get(f"/api/workspaces/{world.ws.slug}/ai-assistant/threads/")
    assert res.status_code == 200
    titles = {t["title"] for t in res.data}
    assert "chat" in titles
    assert "loop" not in titles


def test_thread_create_is_always_chat(world):
    c = _client(world.member)
    res = c.post(
        f"/api/workspaces/{world.ws.slug}/ai-assistant/threads/",
        {"title": "hi", "kind": "loop"},  # kind is ignored
        format="json",
    )
    assert res.status_code == 201
    thread = AssistantThread.objects.get(id=res.data["id"])
    assert thread.kind == ThreadKind.CHAT


def test_owner_can_read_own_loop_thread_messages(world):
    loop_thread = AssistantThread.objects.create(
        workspace=world.ws, user=world.member, kind=ThreadKind.LOOP
    )
    c = _client(world.member)
    res = c.get(f"/api/workspaces/{world.ws.slug}/ai-assistant/threads/{loop_thread.id}/messages/")
    assert res.status_code == 200
