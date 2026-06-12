# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Async SSE stream for assistant turn events.

Plain async Django view (DRF does not do SSE). Auth is the session cookie /
JWT-populated ``request.user``; replays persisted events with ``seq > after``
then subscribes to the thread's Redis channel. Mirrors the runner chat SSE view.
"""

from __future__ import annotations

import json
import logging

from asgiref.sync import sync_to_async
from django.http import HttpResponse, StreamingHttpResponse

from pi_dash.assistant.models import AssistantEvent, AssistantThread
from pi_dash.assistant.runtime.events import event_channel, serialize_event
from pi_dash.core.permissions import ROLE_MEMBER, workspace_role_by_slug
from pi_dash.settings.redis import async_redis_instance

logger = logging.getLogger(__name__)


def _resolve(request, slug, thread_id):
    user = getattr(request, "user", None)
    if user is None or not user.is_authenticated:
        return None, None
    role = workspace_role_by_slug(user, slug)
    if role is None or role < ROLE_MEMBER:
        return user, None
    thread = AssistantThread.objects.filter(
        id=thread_id, user=user, workspace__slug=slug
    ).first()
    return user, thread


def _replay(thread_id, after):
    rows = AssistantEvent.objects.filter(thread_id=thread_id, seq__gt=after).order_by("seq")[:1000]
    return [serialize_event(e) for e in rows]


def _sse(payload: dict) -> str:
    return f"event: chat.event\ndata: {json.dumps(payload, default=str)}\n\n"


async def assistant_event_stream(request, slug, thread_id):
    user, thread = await sync_to_async(_resolve)(request, slug, thread_id)
    if user is None:
        return HttpResponse(status=401)
    if thread is None:
        return HttpResponse(status=404)

    try:
        after = int(request.GET.get("after", 0) or 0)
    except (TypeError, ValueError):
        after = 0

    async def stream():
        last = after
        for ev in await sync_to_async(_replay)(thread_id, after):
            last = ev["seq"]
            yield _sse(ev)

        client = async_redis_instance()
        pubsub = client.pubsub(ignore_subscribe_messages=True)
        await pubsub.subscribe(event_channel(thread_id))
        try:
            while True:
                msg = await pubsub.get_message(timeout=1.0)
                if msg is None:
                    yield ": keepalive\n\n"
                    continue
                data = msg.get("data")
                if data is None:
                    continue
                # data is already the serialized event JSON published by the worker
                yield f"event: chat.event\ndata: {data}\n\n"
        except Exception:
            logger.exception("assistant SSE stream error for thread %s", thread_id)
        finally:
            try:
                await pubsub.unsubscribe(event_channel(thread_id))
            except Exception:
                pass
            try:
                await pubsub.aclose()
            except Exception:
                pass

    response = StreamingHttpResponse(stream(), content_type="text/event-stream")
    response["Cache-Control"] = "no-cache"
    response["X-Accel-Buffering"] = "no"  # disable nginx proxy buffering
    # Setting Content-Encoding makes Django's GZipMiddleware skip this response,
    # so SSE chunks are not held in a gzip buffer (which would batch/delay
    # token deltas). "identity" = no transfer encoding.
    response["Content-Encoding"] = "identity"
    return response
