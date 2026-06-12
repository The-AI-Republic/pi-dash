# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.assistant.models import (
    AssistantEvent,
    AssistantMessage,
    AssistantThread,
    AssistantTurn,
    MessageKind,
)
from pi_dash.assistant.runtime import events

pytestmark = pytest.mark.django_db


@pytest.fixture
def thread(world):
    return AssistantThread.objects.create(workspace=world.ws, user=world.member)


def test_message_seq_increments_per_thread(thread):
    m1 = events.create_message(thread, MessageKind.USER, display_content="a")
    m2 = events.create_message(thread, MessageKind.ASSISTANT, display_content="b")
    assert m1.seq == 1
    assert m2.seq == 2


def test_event_seq_independent_from_message_seq(thread):
    events.create_message(thread, MessageKind.USER, display_content="a")  # message seq 1
    e1 = events.append_event(thread, "turn_started", payload={"x": 1})
    e2 = events.append_event(thread, "assistant_delta", payload={"params": {"delta": "hi"}})
    # event seq starts at 1 independently of message seq
    assert e1.seq == 1
    assert e2.seq == 2


def test_message_envelope_shape(thread):
    msg = events.create_message(thread, MessageKind.ASSISTANT, display_content="hello")
    env = events.message_envelope(msg)
    assert env["role"] == "assistant"  # kind -> role
    assert env["content"] == "hello"  # display_content -> content
    assert set(env.keys()) >= {"id", "role", "content", "status", "seq", "turn_id", "payload", "created_at"}


def test_prune_turn_deltas_only_removes_deltas(thread):
    turn = AssistantTurn.objects.create(thread=thread)
    events.append_event(thread, "assistant_delta", payload={"params": {"delta": "x"}}, turn=turn)
    events.append_event(thread, "assistant_delta", payload={"params": {"delta": "y"}}, turn=turn)
    events.append_event(thread, "turn_completed", payload={}, turn=turn)

    events.prune_turn_deltas(turn)

    kinds = set(AssistantEvent.objects.filter(turn=turn).values_list("kind", flat=True))
    assert "assistant_delta" not in kinds
    assert "turn_completed" in kinds
