# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from django.db import transaction
from rest_framework import status
from rest_framework.response import Response
from rest_framework.throttling import UserRateThrottle

from pi_dash.assistant.errors import MAX_MESSAGE_CHARS, MAX_THREAD_MESSAGES
from pi_dash.assistant.models import (
    AssistantMessage,
    AssistantThread,
    AssistantTurn,
    MessageKind,
    MessageStatus,
    TurnStatus,
)
from pi_dash.assistant.runtime import events
from pi_dash.assistant.runtime.llm import get_config
from pi_dash.assistant.serializers import AssistantThreadSerializer
from pi_dash.assistant.tasks import cancel_key, run_assistant_turn
from pi_dash.assistant.views._base import AssistantBaseView
from pi_dash.settings.redis import redis_instance


class AssistantMessageThrottle(UserRateThrottle):
    scope = "assistant_message"


def _title_from(content: str) -> str:
    first = content.strip().splitlines()[0] if content.strip() else ""
    return first[:60]


class AssistantMessageListCreateEndpoint(AssistantBaseView):
    def get_throttles(self):
        if self.request.method == "POST":
            return [AssistantMessageThrottle()]
        return super().get_throttles()

    def get(self, request, slug, thread_id):
        denied = self.require_member(request, slug)
        if denied:
            return denied
        thread = self.owned_thread(request, slug, thread_id)
        if thread is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)
        after = int(request.query_params.get("after", 0) or 0)
        limit = max(1, min(int(request.query_params.get("limit", 100) or 100), 200))
        qs = AssistantMessage.objects.filter(thread=thread, seq__gt=after).order_by("seq")[:limit]
        return Response([events.message_envelope(m) for m in qs])

    def post(self, request, slug, thread_id):
        denied = self.require_member(request, slug)
        if denied:
            return denied
        thread = self.owned_thread(request, slug, thread_id)
        if thread is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)

        content = (request.data.get("content") or "").strip()
        if not content:
            return Response({"error": "empty_message"}, status=status.HTTP_400_BAD_REQUEST)
        if len(content) > MAX_MESSAGE_CHARS:
            return Response({"error": "message_too_long"}, status=status.HTTP_400_BAD_REQUEST)

        cfg = get_config(request.user)
        if cfg is None or not cfg.has_api_key:
            return Response(
                {"error": "llm_config_missing", "detail": "Configure your AI provider in Settings."},
                status=status.HTTP_422_UNPROCESSABLE_ENTITY,
            )

        with transaction.atomic():
            locked = AssistantThread.objects.select_for_update().get(pk=thread.pk)
            if locked.active_turn_id is not None:
                return Response({"error": "turn_active"}, status=status.HTTP_409_CONFLICT)
            if AssistantMessage.objects.filter(thread=locked).count() >= MAX_THREAD_MESSAGES:
                return Response(
                    {"error": "thread_full", "detail": "Start a new thread."},
                    status=status.HTTP_409_CONFLICT,
                )
            turn = AssistantTurn.objects.create(thread=locked, status=TurnStatus.QUEUED)
            user_msg = events.create_message(
                locked, MessageKind.USER, turn=turn, display_content=content, status=MessageStatus.COMPLETED
            )
            turn.user_message = user_msg
            turn.save(update_fields=["user_message"])
            locked.active_turn = turn
            if not locked.title:
                locked.title = _title_from(content)
            locked.save(update_fields=["active_turn", "title", "updated_at"])

        transaction.on_commit(lambda tid=str(turn.id): run_assistant_turn.delay(tid))

        return Response(
            {
                "turn": {"id": str(turn.id), "status": turn.status},
                "message": events.message_envelope(user_msg),
            },
            status=status.HTTP_202_ACCEPTED,
        )


class AssistantCancelEndpoint(AssistantBaseView):
    def post(self, request, slug, thread_id):
        denied = self.require_member(request, slug)
        if denied:
            return denied
        thread = self.owned_thread(request, slug, thread_id)
        if thread is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)
        if thread.active_turn_id is None:
            return Response({"error": "no_active_turn"}, status=status.HTTP_409_CONFLICT)
        try:
            redis_instance().set(cancel_key(thread.active_turn_id), "1", ex=600)
        except Exception:
            pass
        return Response(status=status.HTTP_204_NO_CONTENT)
