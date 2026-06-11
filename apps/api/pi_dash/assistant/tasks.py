# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Celery turn execution + stale-turn sweep.

One ``asyncio.run`` per task; the model/provider (and its httpx client) is built
inside that loop. ``max_retries=0`` / no ``acks_late``: a crashed turn is
recovered by the sweep and the user retries manually, so write tools are never
re-executed by redelivery. See ``.ai_design/integrate_ai_agent/02-backend.md`` §8.
"""

from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass
from datetime import timedelta

from asgiref.sync import sync_to_async
from celery import shared_task
from celery.exceptions import SoftTimeLimitExceeded
from django.conf import settings
from django.db import transaction
from django.utils import timezone

from pi_dash.assistant.errors import AssistantError
from pi_dash.assistant.models import (
    AssistantMessage,
    AssistantThread,
    AssistantTurn,
    MessageKind,
    MessageStatus,
    TurnStatus,
)
from pi_dash.assistant.runtime import events, history
from pi_dash.assistant.runtime.agent import assistant
from pi_dash.assistant.runtime.deps import AssistantDeps
from pi_dash.core.permissions import workspace_role
from pi_dash.settings.redis import redis_instance

logger = logging.getLogger(__name__)

TURN_SOFT_LIMIT = getattr(settings, "ASSISTANT_TURN_SOFT_LIMIT", 300)
TURN_HARD_LIMIT = getattr(settings, "ASSISTANT_TURN_HARD_LIMIT", 330)
DELTA_FLUSH_MS = 100


class _Cancelled(Exception):
    pass


def cancel_key(turn_id) -> str:
    return f"assistant:cancel:{turn_id}"


@dataclass
class _Ctx:
    thread: AssistantThread
    turn: AssistantTurn
    user: object
    user_text: str
    deps: AssistantDeps


# --------------------------------------------------------------------------- #
# Sync DB helpers (called from async via sync_to_async)
# --------------------------------------------------------------------------- #

def _load_context(turn_id):
    turn = (
        AssistantTurn.objects.select_related("thread", "thread__workspace", "user_message")
        .filter(pk=turn_id)
        .first()
    )
    if turn is None or turn.status not in {TurnStatus.QUEUED, TurnStatus.RUNNING}:
        return None
    thread = turn.thread
    user = thread.user
    role = workspace_role(user, thread.workspace_id) or 0
    user_text = turn.user_message.display_content if turn.user_message_id else ""
    deps = AssistantDeps(
        user_id=user.id,
        user_display=user.display_name or (user.email or ""),
        workspace_id=thread.workspace_id,
        workspace_slug=thread.workspace.slug,
        workspace_name=thread.workspace.name,
        workspace_role=int(role),
        thread_id=thread.id,
        turn_id=turn.id,
    )
    return _Ctx(thread=thread, turn=turn, user=user, user_text=user_text, deps=deps)


def _mark_running(ctx: _Ctx):
    ctx.turn.status = TurnStatus.RUNNING
    ctx.turn.started_at = timezone.now()
    ctx.turn.save(update_fields=["status", "started_at"])
    events.append_event(ctx.thread, "turn_started", payload={"turn_id": str(ctx.turn.id)}, turn=ctx.turn)


def _is_cancelled(turn_id) -> bool:
    try:
        client = redis_instance()
        return bool(client.get(cancel_key(turn_id)))
    except Exception:
        return False


def _start_assistant_row(ctx: _Ctx) -> AssistantMessage:
    msg = events.create_message(
        ctx.thread, MessageKind.ASSISTANT, turn=ctx.turn, status=MessageStatus.STREAMING
    )
    events.append_event(
        ctx.thread,
        "message_created",
        payload={"turn_id": str(ctx.turn.id), "message": events.message_envelope(msg)},
        message=msg,
        turn=ctx.turn,
    )
    return msg


def _emit_delta(ctx: _Ctx, message: AssistantMessage, chunk: str):
    events.append_event(
        ctx.thread,
        "assistant_delta",
        payload={"params": {"delta": chunk}, "turn_id": str(ctx.turn.id)},
        message=message,
        turn=ctx.turn,
    )


def _finalize_row(ctx: _Ctx, message: AssistantMessage, text: str, status: str):
    message.display_content = text
    message.status = status
    message.completed_at = timezone.now()
    message.save(update_fields=["display_content", "status", "completed_at"])
    if status == MessageStatus.COMPLETED:
        events.append_event(
            ctx.thread,
            "message_completed",
            payload={"turn_id": str(ctx.turn.id), "message": events.message_envelope(message)},
            message=message,
            turn=ctx.turn,
        )


def _complete_turn(ctx: _Ctx, model_messages, usage, model_used):
    with transaction.atomic():
        turn = AssistantTurn.objects.select_for_update().get(pk=ctx.turn.id)
        turn.status = TurnStatus.COMPLETED
        turn.model_messages = model_messages
        turn.usage = usage
        turn.model_used = model_used or ""
        turn.completed_at = timezone.now()
        turn.save()
        AssistantThread.objects.filter(pk=ctx.thread.id, active_turn_id=ctx.turn.id).update(active_turn=None)
    events.append_event(
        ctx.thread, "turn_completed", payload={"turn_id": str(ctx.turn.id), "usage": usage or {}}, turn=ctx.turn
    )
    events.prune_turn_deltas(ctx.turn)


def _fail_turn(ctx: _Ctx, code: str, detail: str):
    with transaction.atomic():
        turn = AssistantTurn.objects.select_for_update().get(pk=ctx.turn.id)
        turn.status = TurnStatus.FAILED
        turn.error_code = code[:64]
        turn.error_detail = (detail or "")[:2000]
        turn.completed_at = timezone.now()
        turn.save(update_fields=["status", "error_code", "error_detail", "completed_at"])
        AssistantThread.objects.filter(pk=ctx.thread.id, active_turn_id=ctx.turn.id).update(active_turn=None)
    err = events.create_message(
        ctx.thread, MessageKind.ERROR, turn=ctx.turn, display_content=detail or code, status=MessageStatus.FAILED
    )
    events.append_event(
        ctx.thread,
        "turn_failed",
        payload={"turn_id": str(ctx.turn.id), "error_code": code, "detail": detail or ""},
        message=err,
        turn=ctx.turn,
    )
    events.prune_turn_deltas(ctx.turn)


def _cancel_turn(ctx: _Ctx):
    with transaction.atomic():
        turn = AssistantTurn.objects.select_for_update().get(pk=ctx.turn.id)
        turn.status = TurnStatus.CANCELLED
        turn.completed_at = timezone.now()
        turn.save(update_fields=["status", "completed_at"])
        AssistantThread.objects.filter(pk=ctx.thread.id, active_turn_id=ctx.turn.id).update(active_turn=None)
    events.append_event(ctx.thread, "turn_cancelled", payload={"turn_id": str(ctx.turn.id)}, turn=ctx.turn)
    events.prune_turn_deltas(ctx.turn)


# --------------------------------------------------------------------------- #
# Streaming handler
# --------------------------------------------------------------------------- #

class _Streamer:
    def __init__(self, ctx: _Ctx):
        self.ctx = ctx
        self.message = None
        self.text = ""
        self.pending = ""
        self.last_flush = 0.0

    async def handle(self, run_ctx, event_stream):
        from pydantic_ai.messages import PartDeltaEvent, PartStartEvent, TextPart, TextPartDelta

        async for event in event_stream:
            if await sync_to_async(_is_cancelled)(self.ctx.turn.id):
                raise _Cancelled()
            if isinstance(event, PartStartEvent):
                part = getattr(event, "part", None)
                if isinstance(part, TextPart):
                    await self._finalize()
                    await self._start(getattr(part, "content", "") or "")
                else:
                    await self._finalize()
            elif isinstance(event, PartDeltaEvent):
                delta = getattr(event, "delta", None)
                if isinstance(delta, TextPartDelta):
                    await self._append(getattr(delta, "content_delta", "") or "")
        await self._finalize()

    async def _start(self, initial: str):
        self.message = await sync_to_async(_start_assistant_row)(self.ctx)
        self.text = ""
        self.pending = ""
        self.last_flush = asyncio.get_event_loop().time()
        if initial:
            await self._append(initial)

    async def _append(self, chunk: str):
        if not chunk:
            return
        if self.message is None:
            await self._start("")
        self.text += chunk
        self.pending += chunk
        now = asyncio.get_event_loop().time()
        if (now - self.last_flush) * 1000 >= DELTA_FLUSH_MS or len(self.pending) >= 200:
            await self._flush()

    async def _flush(self):
        if self.message is not None and self.pending:
            chunk, self.pending = self.pending, ""
            await sync_to_async(_emit_delta)(self.ctx, self.message, chunk)
            self.last_flush = asyncio.get_event_loop().time()

    async def _finalize(self):
        if self.message is None:
            return
        await self._flush()
        await sync_to_async(_finalize_row)(self.ctx, self.message, self.text, MessageStatus.COMPLETED)
        self.message = None
        self.text = ""

    async def fail_open_row(self, status: str):
        if self.message is not None:
            await sync_to_async(_finalize_row)(self.ctx, self.message, self.text, status)
            self.message = None


# --------------------------------------------------------------------------- #
# Turn entrypoint
# --------------------------------------------------------------------------- #

async def _run_turn(turn_id: str):
    from pydantic_ai import UsageLimits
    from pydantic_ai.usage import UsageLimitExceeded

    from pi_dash.ee.assistant.model_provider import resolve_model_for_user

    ctx = await sync_to_async(_load_context)(turn_id)
    if ctx is None:
        return
    await sync_to_async(_mark_running)(ctx)

    try:
        model = await sync_to_async(resolve_model_for_user)(ctx.user)
    except AssistantError as exc:
        await sync_to_async(_fail_turn)(ctx, exc.code, exc.detail)
        return

    hist = await sync_to_async(history.load_history)(ctx.thread)
    streamer = _Streamer(ctx)
    model_label = await sync_to_async(_model_label)(ctx.user)

    try:
        result = await assistant.run(
            ctx.user_text,
            model=model,
            deps=ctx.deps,
            message_history=hist,
            usage_limits=UsageLimits(request_limit=25, tool_calls_limit=20),
            event_stream_handler=streamer.handle,
        )
    except _Cancelled:
        await streamer.fail_open_row(MessageStatus.CANCELLED)
        await sync_to_async(_cancel_turn)(ctx)
        return
    except UsageLimitExceeded as exc:
        await streamer.fail_open_row(MessageStatus.FAILED)
        await sync_to_async(_fail_turn)(ctx, "iteration_limit", str(exc))
        return
    except Exception as exc:  # noqa: BLE001 — classify provider failures
        await streamer.fail_open_row(MessageStatus.FAILED)
        code, detail = _classify_error(exc)
        await sync_to_async(_fail_turn)(ctx, code, detail)
        return

    model_messages = await sync_to_async(history.dump_new_messages)(result)
    usage = _extract_usage(result)
    await sync_to_async(_complete_turn)(ctx, model_messages, usage, model_label)


def _model_label(user) -> str:
    from pi_dash.assistant.runtime.llm import get_config, model_label as _ml

    cfg = get_config(user)
    return _ml(cfg) if cfg else ""


def _extract_usage(result) -> dict:
    try:
        u = result.usage()
    except Exception:
        return {}
    return {
        "input_tokens": getattr(u, "input_tokens", None),
        "output_tokens": getattr(u, "output_tokens", None),
        "total_tokens": getattr(u, "total_tokens", None),
        "requests": getattr(u, "requests", None),
        "tool_calls": getattr(u, "tool_calls", None),
    }


def _classify_error(exc: Exception) -> tuple[str, str]:
    text = str(exc).lower()
    if any(s in text for s in ("401", "unauthorized", "api key", "authentication", "invalid_api_key")):
        return "provider_auth_failed", "Your API key was rejected by the provider."
    if any(s in text for s in ("connection", "timeout", "timed out", "unreachable", "could not connect", "name resolution")):
        return "provider_unreachable", "Could not reach the configured provider endpoint."
    if any(s in text for s in ("model_not_found", "does not exist", "unknown model", "no such model")):
        return "model_invalid", "The configured model name was not accepted by the provider."
    logger.exception("assistant turn failed: %s", exc)
    return "internal", "The assistant hit an unexpected error."


# --------------------------------------------------------------------------- #
# Celery tasks
# --------------------------------------------------------------------------- #

@shared_task(
    name="assistant.run_turn",
    acks_late=False,
    max_retries=0,
    soft_time_limit=TURN_SOFT_LIMIT,
    time_limit=TURN_HARD_LIMIT,
)
def run_assistant_turn(turn_id):
    try:
        asyncio.run(_run_turn(str(turn_id)))
    except SoftTimeLimitExceeded:  # pragma: no cover - signal path
        ctx = _load_context(str(turn_id))
        if ctx is not None:
            _fail_turn(ctx, "turn_timeout", "The assistant took too long to respond.")


@shared_task(name="assistant.sweep_stale_turns")
def sweep_stale_turns() -> int:
    cutoff = timezone.now() - timedelta(seconds=TURN_HARD_LIMIT + 60)
    stale = list(
        AssistantTurn.objects.filter(status=TurnStatus.RUNNING, started_at__lt=cutoff).values_list(
            "id", flat=True
        )
    )
    count = 0
    for turn_id in stale:
        ctx = _load_context(str(turn_id))
        if ctx is None:
            continue
        _fail_turn(ctx, "turn_timeout", "The assistant turn did not finish (worker lost).")
        count += 1
    return count
