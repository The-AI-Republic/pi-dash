# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop-mode runtime seam — mode propagation, unattended instructions,
created_via attribution, kind-aware history cap."""

from __future__ import annotations

import datetime
import types

import pytest
from django.utils import timezone

from pi_dash.assistant.models import AssistantThread, AssistantTurn, ThreadKind, TurnStatus
from pi_dash.assistant.runtime import history
from pi_dash.assistant.runtime.deps import AssistantDeps
from pi_dash.assistant.runtime.instructions import dynamic_instructions
from pi_dash.tests.contract.assistant.conftest import make_deps

pytestmark = pytest.mark.django_db


def _ctx(deps):
    return types.SimpleNamespace(deps=deps)


def test_created_via_depends_on_mode(world):
    chat = make_deps(world.member, world.ws, 15)
    loop = AssistantDeps(**{**chat.__dict__, "mode": "loop"})
    assert chat.created_via == "assistant"
    assert loop.created_via == "loop"


def test_loop_instructions_only_in_loop_mode(world):
    chat = make_deps(world.member, world.ws, 15)
    loop = AssistantDeps(**{**chat.__dict__, "mode": "loop"})
    assert "Unattended mode" not in dynamic_instructions(_ctx(chat))
    assert "Unattended mode" in dynamic_instructions(_ctx(loop))


def _deps_with_real_thread(world, *, mode, kind):
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.member, kind=kind)
    turn = AssistantTurn.objects.create(thread=thread, status=TurnStatus.RUNNING)
    base = make_deps(world.member, world.ws, 15, thread_id=thread.id, turn_id=turn.id)
    return AssistantDeps(**{**base.__dict__, "mode": mode})


def test_create_issue_marks_created_via_loop(world):
    from pi_dash.assistant.tools import issues as issue_tools
    from pi_dash.db.models import Issue

    deps = _deps_with_real_thread(world, mode="loop", kind=ThreadKind.LOOP)
    result = issue_tools.create_issue(
        _ctx(deps), project_id=str(world.proj_a.id), name="From the loop"
    )
    issue = Issue.objects.get(id=result["id"])
    assert issue.created_via == "loop"


def test_create_issue_marks_created_via_assistant_in_chat(world):
    from pi_dash.assistant.tools import issues as issue_tools
    from pi_dash.db.models import Issue

    deps = _deps_with_real_thread(world, mode="chat", kind=ThreadKind.CHAT)
    result = issue_tools.create_issue(
        _ctx(deps), project_id=str(world.proj_a.id), name="From chat"
    )
    issue = Issue.objects.get(id=result["id"])
    assert issue.created_via == "assistant"


def test_loop_history_cap(world, settings, mocker):
    settings.ASSISTANT_LOOP_HISTORY_MAX_TURNS = 2
    settings.ASSISTANT_HISTORY_MAX_TURNS = 40
    # Pass stored blobs through verbatim so the test controls their shape.
    mocker.patch(
        "pydantic_ai.messages.ModelMessagesTypeAdapter.validate_python",
        side_effect=lambda blob: blob,
    )
    thread = AssistantThread.objects.create(
        workspace=world.ws, user=world.member, kind=ThreadKind.LOOP
    )
    t0 = timezone.now() - datetime.timedelta(minutes=10)
    for i in range(4):
        turn = AssistantTurn.objects.create(
            thread=thread, status=TurnStatus.COMPLETED, model_messages=[{"i": i}]
        )
        AssistantTurn.objects.filter(pk=turn.pk).update(
            created_at=t0 + datetime.timedelta(minutes=i)
        )

    out = history.load_history(thread)
    # Only the newest 2 turns replayed (loop cap, not the chat cap of 40).
    assert out == [{"i": 2}, {"i": 3}]


def test_chat_history_uses_chat_cap(world, settings, mocker):
    settings.ASSISTANT_LOOP_HISTORY_MAX_TURNS = 2
    settings.ASSISTANT_HISTORY_MAX_TURNS = 40
    mocker.patch(
        "pydantic_ai.messages.ModelMessagesTypeAdapter.validate_python",
        side_effect=lambda blob: blob,
    )
    thread = AssistantThread.objects.create(
        workspace=world.ws, user=world.member, kind=ThreadKind.CHAT
    )
    t0 = timezone.now() - datetime.timedelta(minutes=10)
    for i in range(4):
        turn = AssistantTurn.objects.create(
            thread=thread, status=TurnStatus.COMPLETED, model_messages=[{"i": i}]
        )
        AssistantTurn.objects.filter(pk=turn.pk).update(
            created_at=t0 + datetime.timedelta(minutes=i)
        )
    # Chat cap (40) keeps all 4.
    assert len(history.load_history(thread)) == 4
