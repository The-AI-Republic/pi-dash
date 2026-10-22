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

from pi_dash.utils.db_async import close_idle_connections, db_sync_to_async
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
from pi_dash.assistant.runtime.provider_errors import classify_provider_error
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
# Sync DB helpers (called from async via db_sync_to_async, which closes
# the Django connection around each call so a long agent turn does not park an
# idle Postgres connection for its whole duration).
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
        mode=getattr(thread, "kind", "chat"),
    )
    return _Ctx(thread=thread, turn=turn, user=user, user_text=user_text, deps=deps)


def _mark_running(ctx: _Ctx) -> bool:
    """Atomically move QUEUED→RUNNING. Returns False if already taken/cancelled."""
    updated = AssistantTurn.objects.filter(pk=ctx.turn.id, status=TurnStatus.QUEUED).update(
        status=TurnStatus.RUNNING, started_at=timezone.now()
    )
    if not updated:
        return False
    events.append_event(ctx.thread, "turn_started", payload={"turn_id": str(ctx.turn.id)}, turn=ctx.turn)
    return True


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


def _complete_turn(ctx: _Ctx, model_messages, usage, model_used, *, truncated: bool = False):
    with transaction.atomic():
        turn = AssistantTurn.objects.select_for_update().get(pk=ctx.turn.id)
        turn.status = TurnStatus.COMPLETED
        turn.model_messages = model_messages
        turn.usage = usage
        turn.model_used = model_used or ""
        turn.completed_at = timezone.now()
        turn.save()
        AssistantThread.objects.filter(pk=ctx.thread.id, active_turn_id=ctx.turn.id).update(active_turn=None)
    payload = {"turn_id": str(ctx.turn.id), "usage": usage or {}}
    if truncated:
        payload["truncated"] = True
    events.append_event(ctx.thread, "turn_completed", payload=payload, turn=ctx.turn)
    events.prune_turn_deltas(ctx.turn)


def _finalize_open_rows(turn, status: str):
    """Close any still-streaming assistant rows for a turn (covers the
    soft-time-limit signal and sweep paths, where the in-loop finalizer can't run)."""
    AssistantMessage.objects.filter(turn=turn, status=MessageStatus.STREAMING).update(
        status=status, completed_at=timezone.now()
    )


def _fail_turn(ctx: _Ctx, code: str, detail: str):
    with transaction.atomic():
        turn = AssistantTurn.objects.select_for_update().get(pk=ctx.turn.id)
        turn.status = TurnStatus.FAILED
        turn.error_code = code[:64]
        turn.error_detail = (detail or "")[:2000]
        turn.completed_at = timezone.now()
        turn.save(update_fields=["status", "error_code", "error_detail", "completed_at"])
        _finalize_open_rows(turn, MessageStatus.FAILED)
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
        _finalize_open_rows(turn, MessageStatus.CANCELLED)
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
            if await db_sync_to_async(_is_cancelled)(self.ctx.turn.id):
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
        self.message = await db_sync_to_async(_start_assistant_row)(self.ctx)
        self.text = ""
        self.pending = ""
        self.last_flush = asyncio.get_running_loop().time()
        if initial:
            await self._append(initial)

    async def _append(self, chunk: str):
        if not chunk:
            return
        if self.message is None:
            await self._start("")
        self.text += chunk
        self.pending += chunk
        now = asyncio.get_running_loop().time()
        if (now - self.last_flush) * 1000 >= DELTA_FLUSH_MS or len(self.pending) >= 200:
            await self._flush()

    async def _flush(self):
        if self.message is not None and self.pending:
            chunk, self.pending = self.pending, ""
            # Deliberately a bare ``sync_to_async`` while every other ORM call
            # in this module uses ``db_sync_to_async``. This is the hot
            # streaming path — it fires every DELTA_FLUSH_MS (100ms) for the
            # length of a turn — and ``db_sync_to_async`` closes the
            # connection after each call, so using it here would mean a fresh
            # connect + auth round-trip to the (remote) database ten times a
            # second. The connection this leaves open is the worker's single
            # thread-sensitive one, which is bounded at one per prefork child
            # and released by ``_close_turn_connection`` when the turn ends.
            await sync_to_async(_emit_delta)(self.ctx, self.message, chunk)
            self.last_flush = asyncio.get_running_loop().time()

    async def _finalize(self):
        if self.message is None:
            return
        await self._flush()
        await db_sync_to_async(_finalize_row)(self.ctx, self.message, self.text, MessageStatus.COMPLETED)
        self.message = None
        self.text = ""

    async def fail_open_row(self, status: str):
        if self.message is not None:
            await db_sync_to_async(_finalize_row)(self.ctx, self.message, self.text, status)
            self.message = None


# --------------------------------------------------------------------------- #
# Turn entrypoint
# --------------------------------------------------------------------------- #

async def _run_turn(turn_id: str):
    from pydantic_ai import UsageLimits
    from pydantic_ai.usage import UsageLimitExceeded

    from pi_dash.ee.assistant.model_provider import (
        resolve_model_for_user,
        resolve_toolsets_for_user,
    )

    ctx = await db_sync_to_async(_load_context)(turn_id)
    if ctx is None:
        return
    if not await db_sync_to_async(_mark_running)(ctx):
        return  # turn was already taken, cancelled, or swept

    try:
        model = await db_sync_to_async(resolve_model_for_user)(ctx.user)
    except AssistantError as exc:
        await db_sync_to_async(_fail_turn)(ctx, exc.code, exc.detail)
        return

    # Tool servers are additive: a failure to build them degrades the turn's
    # capabilities but never fails it, so this is deliberately outside the
    # model-resolution try/except above.
    toolsets, skipped = await db_sync_to_async(_resolve_toolsets)(
        ctx, resolve_toolsets_for_user
    )

    hist = await db_sync_to_async(history.load_history)(ctx.thread)
    streamer = _Streamer(ctx)
    model_label = await db_sync_to_async(_model_label)(ctx.user)

    # Servers that dropped out mid-run were absorbed to keep the turn alive;
    # report them however the turn ends. On the failure paths this matters
    # most: a turn that died *because* a tool server misbehaved is exactly when
    # the user needs to know which one, and reporting only on success left that
    # case silent.
    try:
        result = await assistant.run(
            ctx.user_text,
            model=model,
            deps=ctx.deps,
            message_history=hist,
            toolsets=toolsets,
            usage_limits=UsageLimits(request_limit=25, tool_calls_limit=20),
            event_stream_handler=streamer.handle,
        )
    except _Cancelled:
        await db_sync_to_async(_report_runtime_tool_failures)(ctx, toolsets)
        await streamer.fail_open_row(MessageStatus.CANCELLED)
        await db_sync_to_async(_cancel_turn)(ctx)
        return
    except UsageLimitExceeded as exc:
        await db_sync_to_async(_report_runtime_tool_failures)(ctx, toolsets)
        await streamer.fail_open_row(MessageStatus.FAILED)
        await db_sync_to_async(_fail_turn)(ctx, "iteration_limit", str(exc))
        return
    except Exception as exc:  # noqa: BLE001 — classify provider failures
        await db_sync_to_async(_report_runtime_tool_failures)(ctx, toolsets)
        await streamer.fail_open_row(MessageStatus.FAILED)
        code, detail = await db_sync_to_async(_classify_error)(exc, ctx.user)
        await db_sync_to_async(_fail_turn)(ctx, code, detail)
        return

    await db_sync_to_async(_report_runtime_tool_failures)(ctx, toolsets)

    model_messages = await db_sync_to_async(history.dump_new_messages)(result)
    usage = _extract_usage(result)
    truncated = _was_cut_off(result)
    if truncated:
        await db_sync_to_async(_mark_cut_off)(ctx)
    await db_sync_to_async(_complete_turn)(ctx, model_messages, usage, model_label, truncated=truncated)


def _resolve_toolsets(ctx, resolver) -> tuple[list, list]:
    """Build the run's toolsets, tolerating any failure.

    Emits a ``tool_servers_skipped`` event when some servers could not be
    built, so the user learns their tool server is misconfigured instead of
    silently wondering why the assistant "forgot" a capability. A resolver
    that blows up entirely costs the turn its tools, not its life.
    """
    try:
        toolsets, skipped = resolver(ctx.user)
    except Exception:  # noqa: BLE001 — tools are additive; never fail the turn
        logger.exception("resolving assistant toolsets failed")
        # The one case where *every* tool server vanishes must not also be
        # the one case that says nothing — that would invert the per-server
        # skipped reporting below.
        _emit_skipped(ctx, [{"name": "all tool servers", "reason": "toolsets_unavailable"}])
        return [], []

    _emit_skipped(ctx, [{"name": s.name, "reason": s.reason} for s in skipped])
    return toolsets, skipped


def _emit_skipped(ctx, servers: list[dict]) -> None:
    if not servers:
        return
    try:
        events.append_event(
            ctx.thread,
            "tool_servers_skipped",
            payload={"servers": servers},
            turn=ctx.turn,
        )
    except Exception:  # noqa: BLE001 — notification failure must not fail the turn
        logger.exception("emitting tool_servers_skipped failed")


def _report_runtime_tool_failures(ctx, toolsets: list) -> None:
    """Tell the user about servers that died *during* the run.

    Connection happens when the agent enters a toolset, so a server that was
    reachable at build time can still drop out mid-turn. Those failures are
    absorbed to keep the turn alive, which means this is the only place the
    user learns a capability was missing.
    """
    failed = [
        {"name": ts.server_name, "reason": ts.failure}
        for ts in toolsets
        if getattr(ts, "failure", None)
    ]
    _emit_skipped(ctx, failed)


def _model_label(user) -> str:
    from pi_dash.ee.assistant.model_provider import model_label_for_user

    return model_label_for_user(user)


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


def _classify_error(exc: Exception, user=None) -> tuple[str, str]:
    # A typed error raised inside the run — by a model adapter or an ee
    # provider — already carries its stable code and user-facing detail.
    # Guessing from the message text instead would erase both and land it in
    # the generic "unexpected error" bucket. The cause chain is walked because
    # the agent runtime may re-raise a provider failure wrapped in its own type.
    cause: BaseException | None = exc
    while cause is not None:
        if isinstance(cause, AssistantError):
            return cause.code, cause.detail
        cause = cause.__cause__

    typed = _turn_error(exc, user)
    if typed is not None:
        return typed.code, typed.detail
    if _hit_output_limit(exc):
        return "response_truncated", "The reply was cut off because it reached the model's output limit."
    logger.error("assistant turn failed: %s", exc, exc_info=exc)
    return "internal", "The assistant hit an unexpected error."


def _turn_error(exc: Exception, user) -> AssistantError | None:
    """Ask the model-provider seam how this failure should read for ``user``.

    The wording depends on how the user reaches the model (their own key, or a
    metered gateway), which only the seam knows. The hook is looked up rather
    than imported because an overlay replaces that module wholesale: a build
    whose overlay predates the hook must still classify errors, and a hook
    that itself fails must not strand the turn as running.
    """
    from pi_dash.ee.assistant import model_provider

    hook = getattr(model_provider, "turn_error_for_user", None)
    if hook is not None and user is not None:
        try:
            typed = hook(user, exc)
            return typed if isinstance(typed, AssistantError) else None
        except Exception:  # noqa: BLE001 — never fail the failure path
            logger.exception("turn_error_for_user failed; using the default classifier")
    return classify_provider_error(exc)


def _hit_output_limit(exc: Exception) -> bool:
    """True when the model ran out of output tokens before it could reply.

    The runtime raises instead of returning when the limit lands mid-tool-call
    or before any text, so this never reaches :func:`_was_cut_off`.
    """
    from pydantic_ai.exceptions import IncompleteToolCall, UnexpectedModelBehavior

    if isinstance(exc, IncompleteToolCall):
        return True
    return isinstance(exc, UnexpectedModelBehavior) and "token limit" in str(exc).lower()


def _was_cut_off(result) -> bool:
    """True when the final reply ended because the output limit was reached."""
    try:
        return result.response.finish_reason == "length"
    except Exception:  # noqa: BLE001 — a missing field is "not truncated"
        return False


def _mark_cut_off(ctx: _Ctx) -> None:
    """Flag the turn's last reply as cut off, in the row and on the stream.

    The row was already finalized as completed while streaming — the finish
    reason is only known once the whole run returns — so the flag is written
    to its payload and the envelope is sent again for clients to merge.
    """
    message = (
        AssistantMessage.objects.filter(turn=ctx.turn, kind=MessageKind.ASSISTANT).order_by("-seq").first()
    )
    if message is None:
        return
    message.payload = {**(message.payload or {}), "truncated": True}
    message.save(update_fields=["payload"])
    events.append_event(
        ctx.thread,
        "message_completed",
        payload={"turn_id": str(ctx.turn.id), "message": events.message_envelope(message)},
        message=message,
        turn=ctx.turn,
    )


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
    finally:
        asyncio.run(_close_turn_connection())


async def _close_turn_connection() -> None:
    """Release the worker's thread-sensitive DB connection at turn end.

    ``AssistantStream._flush`` deliberately leaves a connection open on the
    worker's thread-sensitive thread rather than reconnecting ten times a
    second. Close it here so it does not sit idle between turns — a Celery
    child can be parked for hours, and these connections share a Postgres
    host with home-page.

    This has to run *on that thread*: Django connections are thread-local, so
    calling ``close_idle_connections()`` from the Celery main thread would not
    reach it. ``sync_to_async(thread_sensitive=True)`` routes back to the same
    process-wide executor the turn used, even from this second event loop.
    """
    await sync_to_async(close_idle_connections)()


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
