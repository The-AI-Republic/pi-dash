# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from datetime import timedelta
import types

import pytest
from django.utils import timezone
from rest_framework.test import APIClient

from pi_dash.assistant.models import AssistantThread, AssistantTurn
from pi_dash.tests.contract.assistant.conftest import configure_llm

pytestmark = pytest.mark.django_db


def client_for(user):
    c = APIClient()
    c.force_authenticate(user=user)
    return c


def base(ws):
    return f"/api/workspaces/{ws.slug}/ai-assistant"


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


def test_listing_threads_reaps_abandoned_empty_conversations(world):
    """An untitled chat thread with no turns (no history) is reaped once past
    the grace window, while fresh empties, titled threads, and threads with
    history survive."""
    stale = timezone.now() - timedelta(hours=2)

    def make(title="", *, old=False, with_turn=False):
        t = AssistantThread.objects.create(workspace=world.ws, user=world.member, title=title)
        if old:
            AssistantThread.objects.filter(pk=t.pk).update(created_at=stale)
        if with_turn:
            AssistantTurn.objects.create(thread=t)
        return t

    empty_old = make(old=True)  # reaped
    empty_fresh = make()  # kept (grace)
    titled_old = make("Kept", old=True)  # kept (has title)
    history_old = make(old=True, with_turn=True)  # kept (has history)

    res = client_for(world.member).get(f"{base(world.ws)}/threads/")
    assert res.status_code == 200

    assert not AssistantThread.objects.filter(pk=empty_old.pk).exists()
    for kept in (empty_fresh, titled_old, history_old):
        assert AssistantThread.objects.filter(pk=kept.pk).exists()


# --- messages ---


def test_message_requires_llm_config(world, kms_crypto):
    c = client_for(world.member)
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.member)
    res = c.post(f"{base(world.ws)}/threads/{thread.id}/messages/", {"content": "hello"}, format="json")
    assert res.status_code == 422
    assert res.data["error"] == "llm_config_missing"


def test_message_creates_turn_and_blocks_concurrent(world, kms_crypto, mocker, django_capture_on_commit_callbacks):
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


def test_message_lists_in_envelope_shape(world, kms_crypto, mocker):
    mocker.patch("pi_dash.assistant.views.messages.run_assistant_turn.delay")
    configure_llm(world.member)
    c = client_for(world.member)
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.member)
    c.post(f"{base(world.ws)}/threads/{thread.id}/messages/", {"content": "hi"}, format="json")

    res = c.get(f"{base(world.ws)}/threads/{thread.id}/messages/")
    assert res.status_code == 200
    assert res.data[0]["role"] == "user"
    assert "content" in res.data[0]


def test_message_list_paginates_and_tolerates_bad_params(world, kms_crypto):
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


def test_cannot_access_other_users_thread(world, kms_crypto):
    other_thread = AssistantThread.objects.create(workspace=world.ws, user=world.admin)
    c = client_for(world.member)
    res = c.get(f"{base(world.ws)}/threads/{other_thread.id}/messages/")
    assert res.status_code == 404


# --- llm config ---


def test_llm_config_lifecycle(world, kms_crypto):
    c = client_for(world.member)
    # unset -> 200 with has_api_key False
    res = c.get("/api/users/me/ai-assistant/config/")
    assert res.status_code == 200
    assert res.data["has_api_key"] is False

    # set
    res = c.put(
        "/api/users/me/ai-assistant/config/",
        {
            "provider_kind": "openai_compatible",
            "base_url": "https://api.example.com/v1",
            "model_name": "m",
            "api_key": "sk-12345678",
        },
        format="json",
    )
    assert res.status_code == 200
    assert res.data["has_api_key"] is True

    # the key is never returned
    assert "api_key" not in res.data

    # delete
    assert c.delete("/api/users/me/ai-assistant/config/").status_code == 204
    assert c.get("/api/users/me/ai-assistant/config/").data["has_api_key"] is False


# --- generate title ---


def gen_title_url(ws):
    return f"{base(ws)}/generate-title/"


def test_generate_title_requires_llm_config(world, kms_crypto):
    c = client_for(world.member)
    res = c.post(gen_title_url(world.ws), {"description": "Ship the new dashboard export."}, format="json")
    assert res.status_code == 422
    assert res.data["error"] == "llm_config_missing"


def test_generate_title_requires_description(world, kms_crypto):
    configure_llm(world.member)
    c = client_for(world.member)
    res = c.post(gen_title_url(world.ws), {"description": "   "}, format="json")
    assert res.status_code == 400
    assert res.data["error"] == "description_required"


def test_generate_title_returns_generated_title(world, kms_crypto, mocker):
    mocker.patch(
        "pi_dash.assistant.views.llm_config.generate_title_for_user",
        return_value="Add dashboard CSV export",
    )
    configure_llm(world.member)
    c = client_for(world.member)
    res = c.post(
        gen_title_url(world.ws),
        {"description": "Users want to download the dashboard data as a CSV file."},
        format="json",
    )
    assert res.status_code == 200
    assert res.data["title"] == "Add dashboard CSV export"


def test_generate_title_uses_direct_openai_compatible_call(world, kms_crypto, monkeypatch):
    calls = []

    class FakeCompletions:
        def create(self, **kwargs):
            calls.append(kwargs)
            return types.SimpleNamespace(
                choices=[types.SimpleNamespace(message=types.SimpleNamespace(content='"Add dashboard CSV export."'))]
            )

    class FakeOpenAI:
        def __init__(self, **kwargs):
            calls.append({"client": kwargs})
            self.chat = types.SimpleNamespace(completions=FakeCompletions())

    monkeypatch.setattr("openai.OpenAI", FakeOpenAI)
    configure_llm(world.member)
    c = client_for(world.member)

    res = c.post(
        gen_title_url(world.ws),
        {"description": "Users want to download the dashboard data as a CSV file."},
        format="json",
    )

    assert res.status_code == 200
    assert res.data["title"] == "Add dashboard CSV export"
    assert calls[0] == {
        "client": {
            "api_key": "sk-test-key-123456",
            "base_url": "https://api.example.com/v1",
            "timeout": 20.0,
        }
    }
    assert calls[1]["model"] == "gpt-test"
    assert calls[1]["max_tokens"] == 256
    assert calls[1]["messages"][0]["role"] == "system"
    assert calls[1]["messages"][1]["content"] == "Users want to download the dashboard data as a CSV file."
    assert "extra_body" not in calls[1]


def test_generate_title_disables_deepseek_thinking_mode(world, kms_crypto, monkeypatch):
    calls = []

    class FakeCompletions:
        def create(self, **kwargs):
            calls.append(kwargs)
            return types.SimpleNamespace(
                choices=[types.SimpleNamespace(message=types.SimpleNamespace(content='"Add dashboard CSV export."'))]
            )

    class FakeOpenAI:
        def __init__(self, **kwargs):
            self.chat = types.SimpleNamespace(completions=FakeCompletions())

    monkeypatch.setattr("openai.OpenAI", FakeOpenAI)
    configure_llm(world.member, model="deepseek-v4-pro", base_url="https://api.deepseek.com")
    c = client_for(world.member)

    res = c.post(
        gen_title_url(world.ws),
        {"description": "Users want to download the dashboard data as a CSV file."},
        format="json",
    )

    assert res.status_code == 200
    assert res.data["title"] == "Add dashboard CSV export"
    assert calls[0]["extra_body"] == {"thinking": {"type": "disabled"}}


def test_generate_title_strips_reasoning_wrappers(world, kms_crypto, monkeypatch):
    class FakeCompletions:
        def create(self, **kwargs):
            return types.SimpleNamespace(
                choices=[
                    types.SimpleNamespace(
                        message=types.SimpleNamespace(
                            content="<think>Need a concise title.</think>\nTitle: Add dashboard CSV export."
                        )
                    )
                ]
            )

    class FakeOpenAI:
        def __init__(self, **kwargs):
            self.chat = types.SimpleNamespace(completions=FakeCompletions())

    monkeypatch.setattr("openai.OpenAI", FakeOpenAI)
    configure_llm(world.member)
    c = client_for(world.member)

    res = c.post(
        gen_title_url(world.ws),
        {"description": "Users want to download the dashboard data as a CSV file."},
        format="json",
    )

    assert res.status_code == 200
    assert res.data["title"] == "Add dashboard CSV export"


def test_generate_title_ignores_structured_reasoning_blocks(world, kms_crypto, monkeypatch):
    class FakeCompletions:
        def create(self, **kwargs):
            return types.SimpleNamespace(
                choices=[
                    types.SimpleNamespace(
                        message=types.SimpleNamespace(
                            content=[
                                {"type": "reasoning", "text": "Need a concise title."},
                                {"type": "text", "text": "Add dashboard CSV export"},
                            ]
                        )
                    )
                ]
            )

    class FakeOpenAI:
        def __init__(self, **kwargs):
            self.chat = types.SimpleNamespace(completions=FakeCompletions())

    monkeypatch.setattr("openai.OpenAI", FakeOpenAI)
    configure_llm(world.member)
    c = client_for(world.member)

    res = c.post(
        gen_title_url(world.ws),
        {"description": "Users want to download the dashboard data as a CSV file."},
        format="json",
    )

    assert res.status_code == 200
    assert res.data["title"] == "Add dashboard CSV export"


def test_generate_title_reports_provider_failure(world, kms_crypto, mocker):
    mocker.patch(
        "pi_dash.assistant.views.llm_config.generate_title_for_user",
        side_effect=RuntimeError("boom"),
    )
    configure_llm(world.member)
    c = client_for(world.member)
    res = c.post(gen_title_url(world.ws), {"description": "Something to summarize."}, format="json")
    assert res.status_code == 502
    assert res.data["error"] == "provider_unreachable"


def test_generate_title_blocked_for_guest(world, kms_crypto):
    c = client_for(world.guest)
    res = c.post(gen_title_url(world.ws), {"description": "Ship the new dashboard export."}, format="json")
    assert res.status_code == 403
