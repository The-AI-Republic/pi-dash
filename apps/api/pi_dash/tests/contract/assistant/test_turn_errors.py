# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""How a failed model run reaches the user.

A model adapter (in-tree or an ee provider) can raise a typed
``AssistantError`` from inside the run. The contract is that its stable code
and user-facing detail survive to the ``turn_failed`` event, instead of being
re-guessed from the message text.
"""

from __future__ import annotations

import pytest

from pi_dash.assistant import tasks as assistant_tasks
from pi_dash.assistant.errors import ProviderUnreachable, QuotaExceeded
from pi_dash.assistant.models import (
    AssistantEvent,
    AssistantMessage,
    AssistantThread,
    AssistantTurn,
    MessageKind,
    MessageStatus,
    TurnStatus,
)
from pi_dash.assistant.runtime import events

USAGE_DETAIL = "Plan usage is unavailable. Check Settings → Usage."


def test_typed_errors_keep_their_code_and_detail():
    assert assistant_tasks._classify_error(QuotaExceeded(USAGE_DETAIL)) == (
        "quota_exceeded",
        USAGE_DETAIL,
    )
    # Without the typed branch this message matches no text marker at all.
    assert assistant_tasks._classify_error(ProviderUnreachable("Stream ended early.")) == (
        "provider_unreachable",
        "Stream ended early.",
    )


def test_a_typed_error_wrapped_by_the_runtime_is_still_recognised():
    try:
        try:
            raise QuotaExceeded(USAGE_DETAIL)
        except QuotaExceeded as inner:
            raise RuntimeError("model request failed") from inner
    except RuntimeError as wrapped:
        assert assistant_tasks._classify_error(wrapped) == ("quota_exceeded", USAGE_DETAIL)


def test_untyped_errors_are_still_classified_by_text():
    code, _ = assistant_tasks._classify_error(Exception("Connection timed out"))
    assert code == "provider_unreachable"


@pytest.mark.django_db(transaction=True)
def test_typed_error_after_text_deltas_reaches_the_turn_failed_event(world, monkeypatch):
    """The whole turn runner, not just the classifier: the provider streams
    some text and *then* fails with a typed error."""
    from pydantic_ai.models.function import FunctionModel

    from pi_dash.ee.assistant import model_provider

    async def stream_then_fail(messages, info):
        yield "Hello"
        raise QuotaExceeded(USAGE_DETAIL)

    monkeypatch.setattr(
        model_provider,
        "resolve_model_for_user",
        lambda user: FunctionModel(stream_function=stream_then_fail),
    )
    monkeypatch.setattr(model_provider, "resolve_toolsets_for_user", lambda user: ([], []))
    monkeypatch.setattr(model_provider, "model_label_for_user", lambda user: "test-model")
    # No Redis in the turn: nothing to publish to, and nobody cancelled.
    monkeypatch.setattr(events, "publish_event", lambda event: None)
    monkeypatch.setattr(assistant_tasks, "_is_cancelled", lambda turn_id: False)

    thread = AssistantThread.objects.create(workspace=world.ws, user=world.member)
    user_message = AssistantMessage.objects.create(
        thread=thread, seq=1, kind=MessageKind.USER, display_content="Hi"
    )
    turn = AssistantTurn.objects.create(thread=thread, user_message=user_message)
    AssistantThread.objects.filter(pk=thread.pk).update(active_turn=turn)

    assistant_tasks.run_assistant_turn(str(turn.id))

    failed = AssistantEvent.objects.get(thread=thread, kind="turn_failed")
    assert failed.payload == {
        "turn_id": str(turn.id),
        "error_code": "quota_exceeded",
        "detail": USAGE_DETAIL,
    }

    turn.refresh_from_db()
    assert turn.status == TurnStatus.FAILED
    assert turn.error_code == "quota_exceeded"
    assert turn.error_detail == USAGE_DETAIL

    # The text that had already streamed is closed out as failed, and the
    # user-visible error row carries the actionable detail.
    partial = AssistantMessage.objects.get(thread=thread, kind=MessageKind.ASSISTANT)
    assert partial.status == MessageStatus.FAILED
    error_row = AssistantMessage.objects.get(thread=thread, kind=MessageKind.ERROR)
    assert error_row.display_content == USAGE_DETAIL

    thread.refresh_from_db()
    assert thread.active_turn_id is None
