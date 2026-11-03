# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""How a failed model run reaches the user.

A model adapter (in-tree or an ee provider) can raise a typed
``AssistantError`` from inside the run. The contract is that its stable code
and user-facing detail survive to the ``turn_failed`` event, instead of being
re-guessed from the message text.

Untyped provider failures are worded by the ``ee`` model-provider seam
(``turn_error_for_user``), because only it knows whether the user reaches the
model with their own key or through a metered gateway. A reply the model cut
short at its output limit is not a failure at all, but is flagged so the user
is not left reading half an answer.
"""

from __future__ import annotations

from contextlib import asynccontextmanager

import pytest
from pydantic_ai.exceptions import ModelHTTPError

from pi_dash.assistant import tasks as assistant_tasks
from pi_dash.assistant.errors import AssistantError, ProviderUnreachable, QuotaExceeded
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
from pi_dash.assistant.runtime.provider_errors import GATEWAY, classify_provider_error

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


def test_without_a_user_the_byok_wording_applies():
    assert assistant_tasks._classify_error(ModelHTTPError(401, "m", {"message": "Invalid JWT"})) == (
        "provider_auth_failed",
        "Your API key was rejected by the provider.",
    )


def test_an_output_limit_hit_before_any_reply_is_reported_as_a_cut_off():
    from pydantic_ai.exceptions import IncompleteToolCall

    code, detail = assistant_tasks._classify_error(
        IncompleteToolCall("Model token limit (1024) exceeded while generating a tool call")
    )
    assert code == "response_truncated"
    assert "output limit" in detail


def _run_turn(world, monkeypatch, model):
    """Run one assistant turn for ``world.member`` against ``model``."""
    from pi_dash.ee.assistant import model_provider

    monkeypatch.setattr(model_provider, "resolve_model_for_user", lambda user: model)
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
    return thread, turn


def _failing_model(exc: Exception):
    from pydantic_ai.models.function import FunctionModel

    async def fail(messages, info):
        raise exc
        yield ""  # pragma: no cover — makes this an async generator

    return FunctionModel(stream_function=fail)


def _failure(thread) -> tuple[str, str]:
    payload = AssistantEvent.objects.get(thread=thread, kind="turn_failed").payload
    return payload["error_code"], payload["detail"]


@pytest.mark.django_db(transaction=True)
def test_typed_error_after_text_deltas_reaches_the_turn_failed_event(world, monkeypatch):
    """The whole turn runner, not just the classifier: the provider streams
    some text and *then* fails with a typed error."""
    from pydantic_ai.models.function import FunctionModel

    async def stream_then_fail(messages, info):
        yield "Hello"
        raise QuotaExceeded(USAGE_DETAIL)

    thread, turn = _run_turn(world, monkeypatch, FunctionModel(stream_function=stream_then_fail))

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


UNSUPPORTED_MODEL = ModelHTTPError(
    400, "acme/gone-1", {"message": "Unsupported model", "type": "invalid_request_error"}
)
WALLET_DETAIL = "Your wallet is empty. Top up at https://hub.example/billing."


@pytest.mark.django_db(transaction=True)
def test_ce_words_a_provider_refusal_for_a_user_supplied_key(world, monkeypatch):
    thread, _ = _run_turn(world, monkeypatch, _failing_model(ModelHTTPError(401, "m", {"message": "Invalid JWT"})))

    assert _failure(thread) == ("provider_auth_failed", "Your API key was rejected by the provider.")


@pytest.mark.django_db(transaction=True)
def test_an_overlay_provider_supplies_the_code_and_detail(world, monkeypatch):
    """The seam an overlay uses to word failures for a gateway connection."""
    from pi_dash.ee.assistant import model_provider

    seen = []

    def gateway_hook(user, exc):
        seen.append(user)
        if getattr(exc, "status_code", None) == 402:
            return AssistantError(WALLET_DETAIL, code="hub_wallet_empty")
        return classify_provider_error(exc, connection=GATEWAY)

    monkeypatch.setattr(model_provider, "turn_error_for_user", gateway_hook)

    thread, _ = _run_turn(world, monkeypatch, _failing_model(UNSUPPORTED_MODEL))
    assert _failure(thread) == (
        "model_unavailable",
        'The model "acme/gone-1" is not available right now. Pick another model in Settings.',
    )
    assert seen == [world.member]

    thread, turn = _run_turn(world, monkeypatch, _failing_model(ModelHTTPError(402, "m", {"message": "no credit"})))
    assert _failure(thread) == ("hub_wallet_empty", WALLET_DETAIL)
    error_row = AssistantMessage.objects.get(thread=thread, kind=MessageKind.ERROR)
    assert error_row.display_content == WALLET_DETAIL


@pytest.mark.django_db(transaction=True)
def test_an_overlay_without_the_hook_still_classifies(world, monkeypatch):
    """An overlay replaces the provider module wholesale; one that predates
    the hook must not turn every provider failure into an import error."""
    from pi_dash.ee.assistant import model_provider

    monkeypatch.delattr(model_provider, "turn_error_for_user")

    thread, _ = _run_turn(world, monkeypatch, _failing_model(ModelHTTPError(429, "m", {"message": "slow down"})))

    assert _failure(thread)[0] == "provider_rate_limited"


@pytest.mark.django_db(transaction=True)
def test_a_failing_hook_does_not_strand_the_turn(world, monkeypatch):
    from pi_dash.ee.assistant import model_provider

    def broken(user, exc):
        raise RuntimeError("overlay bug")

    monkeypatch.setattr(model_provider, "turn_error_for_user", broken)

    thread, turn = _run_turn(world, monkeypatch, _failing_model(ModelHTTPError(429, "m", {"message": "slow down"})))

    assert _failure(thread)[0] == "provider_rate_limited"
    turn.refresh_from_db()
    assert turn.status == TurnStatus.FAILED


def _model_finishing_with(reason: str):
    from pydantic_ai.models.function import FunctionModel

    async def reply(messages, info):
        yield "The answer starts here and"

    class _Model(FunctionModel):
        @asynccontextmanager
        async def request_stream(self, *args, **kwargs):
            async with super().request_stream(*args, **kwargs) as stream:
                stream.finish_reason = reason
                yield stream

    return _Model(stream_function=reply)


@pytest.mark.django_db(transaction=True)
def test_a_reply_that_hit_the_output_limit_is_marked_as_cut_off(world, monkeypatch):
    thread, turn = _run_turn(world, monkeypatch, _model_finishing_with("length"))

    turn.refresh_from_db()
    assert turn.status == TurnStatus.COMPLETED

    # Durable: the transcript row carries the flag, so a reload still shows it.
    reply = AssistantMessage.objects.get(thread=thread, kind=MessageKind.ASSISTANT)
    assert reply.status == MessageStatus.COMPLETED
    assert reply.display_content == "The answer starts here and"
    assert reply.payload == {"truncated": True}

    # Live: the row is re-sent with the flag, and the turn says so too.
    resent = AssistantEvent.objects.filter(thread=thread, kind="message_completed").order_by("seq").last()
    assert resent.payload["message"]["payload"] == {"truncated": True}
    completed = AssistantEvent.objects.get(thread=thread, kind="turn_completed")
    assert completed.payload["truncated"] is True


@pytest.mark.django_db(transaction=True)
def test_a_reply_that_finished_normally_is_not_marked(world, monkeypatch):
    thread, _ = _run_turn(world, monkeypatch, _model_finishing_with("stop"))

    reply = AssistantMessage.objects.get(thread=thread, kind=MessageKind.ASSISTANT)
    assert reply.payload == {}
    assert AssistantEvent.objects.filter(thread=thread, kind="message_completed").count() == 1
    assert "truncated" not in AssistantEvent.objects.get(thread=thread, kind="turn_completed").payload
