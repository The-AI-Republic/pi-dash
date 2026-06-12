# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Event/message persistence + Redis publish — the SSE delivery spine.

Copied from the proven runner-chat pattern (``runner/services/chat.py``):
synchronous publish from a transaction commit hook, async subscribe in the SSE
view. ``seq`` is allocated MAX+1 under a row lock on the thread so concurrent
writers (streaming handler + threadpool tools) don't collide. See
``.ai_design/integrate_ai_agent/02-backend.md`` §8.
"""

from __future__ import annotations

import json
import logging
from typing import Any, Optional

from django.db import transaction
from django.db.models import Max

from pi_dash.assistant.models import (
    AssistantEvent,
    AssistantMessage,
    AssistantThread,
    AssistantTurn,
    MessageStatus,
)
from pi_dash.settings.redis import redis_instance

logger = logging.getLogger(__name__)

CHANNEL_PREFIX = "assistant:thread:"


def event_channel(thread_id) -> str:
    return f"{CHANNEL_PREFIX}{thread_id}"


def serialize_event(event: AssistantEvent) -> dict[str, Any]:
    return {
        "id": event.id,
        "thread": str(event.thread_id),
        "message": str(event.message_id) if event.message_id else None,
        "seq": event.seq,
        "kind": event.kind,
        "payload": event.payload,
        "created_at": event.created_at.isoformat(),
    }


def publish_event(event: AssistantEvent) -> None:
    try:
        client = redis_instance()
    except Exception:
        logger.exception("assistant: redis unavailable for publish (thread %s)", event.thread_id)
        return
    if client is None:
        return
    try:
        client.publish(event_channel(event.thread_id), json.dumps(serialize_event(event), default=str))
    except Exception:
        logger.exception("assistant: publish failed for thread %s", event.thread_id)


def _publish_by_id(event_id: int) -> None:
    event = AssistantEvent.objects.filter(pk=event_id).first()
    if event is not None:
        publish_event(event)


def _next_event_seq(thread: AssistantThread) -> int:
    current = AssistantEvent.objects.filter(thread=thread).aggregate(Max("seq"))["seq__max"] or 0
    return int(current) + 1


def _next_message_seq(thread: AssistantThread) -> int:
    current = AssistantMessage.objects.filter(thread=thread).aggregate(Max("seq"))["seq__max"] or 0
    return int(current) + 1


def append_event(
    thread: AssistantThread,
    kind: str,
    *,
    payload: Optional[dict[str, Any]] = None,
    message: Optional[AssistantMessage] = None,
    turn: Optional[AssistantTurn] = None,
) -> AssistantEvent:
    """Persist an event (seq-allocated under a thread lock) and publish on commit."""
    with transaction.atomic():
        locked = AssistantThread.objects.select_for_update().get(pk=thread.pk)
        event = AssistantEvent.objects.create(
            thread=locked,
            turn=turn,
            seq=_next_event_seq(locked),
            kind=kind[:64],
            message_id=message.id if message is not None else None,
            payload=payload or {},
        )
        transaction.on_commit(lambda eid=event.id: _publish_by_id(eid))
    return event


def create_message(
    thread: AssistantThread,
    kind: str,
    *,
    turn: Optional[AssistantTurn] = None,
    display_content: str = "",
    payload: Optional[dict[str, Any]] = None,
    status: str = MessageStatus.COMPLETED,
) -> AssistantMessage:
    with transaction.atomic():
        locked = AssistantThread.objects.select_for_update().get(pk=thread.pk)
        return AssistantMessage.objects.create(
            thread=locked,
            turn=turn,
            seq=_next_message_seq(locked),
            kind=kind,
            display_content=display_content,
            payload=payload or {},
            status=status,
        )


def message_envelope(message: AssistantMessage) -> dict[str, Any]:
    """Wire shape consumed by the frontend chat kit (kind -> role, etc.)."""
    return {
        "id": str(message.id),
        "role": message.kind,
        "content": message.display_content,
        "status": message.status,
        "seq": message.seq,
        "turn_id": str(message.turn_id) if message.turn_id else None,
        "payload": message.payload,
        "created_at": message.created_at.isoformat(),
        "completed_at": message.completed_at.isoformat() if message.completed_at else None,
    }


def prune_turn_deltas(turn: AssistantTurn) -> None:
    """Delete a finished turn's delta events; completed content lives in rows."""
    AssistantEvent.objects.filter(turn=turn, kind="assistant_delta").delete()
