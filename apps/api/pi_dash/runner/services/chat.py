# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import json
import logging
from datetime import timedelta
from typing import Any, Optional

from django.db import IntegrityError, transaction
from django.db.models import Max, Q
from django.utils import timezone

from pi_dash.runner.models import (
    AgentChatApprovalRequest,
    AgentChatEvent,
    AgentChatMessage,
    AgentChatMessageRole,
    AgentChatMessageStatus,
    AgentChatSession,
    AgentChatSessionStatus,
    ChatMessageDedupe,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import matcher
from pi_dash.runner.services.permissions import (
    can_use_runner,
    can_view_runner,
    is_workspace_admin,
    is_workspace_member,
)
from pi_dash.runner.services.pubsub import send_to_runner
from pi_dash.settings.redis import redis_instance

logger = logging.getLogger(__name__)

CHAT_EVENT_CHANNEL_PREFIX = "agent_chat_session:"
CHAT_ACTIVE_TIMEOUT_SECS = 1800


def event_channel(session_id) -> str:
    return f"{CHAT_EVENT_CHANNEL_PREFIX}{session_id}"


def can_read_chat(user, session: AgentChatSession) -> bool:
    if not is_workspace_member(user, session.workspace_id):
        return False
    if not can_view_runner(user, session.runner):
        return False
    return session.created_by_id == user.id or is_workspace_admin(user, session.workspace_id)


def can_send_chat(user, session: AgentChatSession) -> bool:
    if not is_workspace_member(user, session.workspace_id):
        return False
    return session.created_by_id == user.id and can_use_runner(user, session.runner)


def can_decide_chat_approval(user, approval: AgentChatApprovalRequest) -> bool:
    session = approval.session
    if not is_workspace_member(user, session.workspace_id):
        return False
    if not can_view_runner(user, session.runner):
        return False
    return session.created_by_id == user.id or is_workspace_admin(user, session.workspace_id)


def runner_has_active_chat(runner: Runner) -> bool:
    return (
        AgentChatSession.objects.filter(
            runner=runner,
            status=AgentChatSessionStatus.OPEN,
        )
        .filter(Q(active_message_id__isnull=False) | ~Q(active_turn_id=""))
        .exists()
    )


def drain_tasks_after_chat_release(session: AgentChatSession) -> None:
    """Wake the task queue after a chat turn finishes (opportunistic).

    Chat and task runs intentionally do not share a queue, and (as of the
    concurrent chat/issue change) an active chat turn no longer makes the
    matcher skip a runner — the two run concurrently. This drain is therefore an
    optimisation, not an unblocking mechanism: a finishing chat turn may free a
    worktree, so it's a cheap moment to re-check whether the runner can pick up
    queued AgentRun rows sooner.
    """

    runner_id = session.runner_id
    pod_id = session.pod_id
    if runner_id is None:
        return

    def _drain() -> None:
        try:
            matcher.drain_for_runner_by_id(runner_id)
            if pod_id is not None:
                matcher.drain_pod_by_id(pod_id)
        except Exception:
            logger.exception(
                "failed to drain task queue after chat release for runner %s",
                runner_id,
            )

    transaction.on_commit(_drain)


def normalize_cwd(value: Any) -> str:
    # MVP: the cloud ignores caller-selected cwd. The runner resolves its
    # configured workspace path and enforces containment before spawn.
    return ""


def next_message_seq_locked(session: AgentChatSession) -> int:
    current = AgentChatMessage.objects.filter(session=session).aggregate(Max("seq"))["seq__max"] or 0
    return int(current) + 1


def next_event_seq_locked(session: AgentChatSession) -> int:
    current = AgentChatEvent.objects.filter(session=session).aggregate(Max("seq"))["seq__max"] or 0
    return int(current) + 1


def serialize_event(event: AgentChatEvent) -> dict[str, Any]:
    return {
        "id": event.id,
        "session": str(event.session_id),
        "message": str(event.message_id) if event.message_id else None,
        "seq": event.seq,
        "kind": event.kind,
        "payload": event.payload,
        "created_at": event.created_at.isoformat(),
    }


def publish_event(event: AgentChatEvent) -> None:
    client = redis_instance()
    if client is None:
        return
    try:
        client.publish(event_channel(event.session_id), json.dumps(serialize_event(event), default=str))
    except Exception:
        logger.exception("publish chat event failed for session %s", event.session_id)


def append_event_locked(
    session: AgentChatSession,
    kind: str,
    payload: Optional[dict[str, Any]] = None,
    *,
    message: Optional[AgentChatMessage] = None,
    source_key: str = "",
) -> AgentChatEvent:
    if source_key:
        existing = AgentChatEvent.objects.filter(session=session, source_key=source_key).first()
        if existing is not None:
            return existing
    event = AgentChatEvent.objects.create(
        session=session,
        message=message,
        seq=next_event_seq_locked(session),
        source_key=source_key[:160],
        kind=kind[:64],
        payload=payload or {},
    )
    transaction.on_commit(lambda eid=event.id: _publish_event_by_id(eid))
    return event


def _publish_event_by_id(event_id: int) -> None:
    event = AgentChatEvent.objects.filter(pk=event_id).first()
    if event is not None:
        publish_event(event)


def record_dedupe(session: AgentChatSession, key: str) -> bool:
    if not key:
        return True
    try:
        ChatMessageDedupe.objects.create(
            session=session,
            message_id=key[:128],
        )
        return True
    except IntegrityError:
        return False


def enqueue_chat_message_after_commit(
    runner_id,
    *,
    chat_session_id,
    message_id,
    content: str,
    content_parts: list[Any],
    local_thread_id: str,
    local_session_id: str,
    cwd: str,
    model: str,
) -> None:
    def _send():
        try:
            send_to_runner(
                runner_id,
                {
                    "type": "chat_user_message",
                    "chat_session_id": str(chat_session_id),
                    "message_id": str(message_id),
                    "content": content,
                    "content_parts": content_parts,
                    "local_thread_id": local_thread_id or None,
                    "local_session_id": local_session_id or None,
                    "cwd": cwd or None,
                    "model": model or None,
                },
            )
        except Exception as exc:
            mark_message_dispatch_failed(chat_session_id, message_id, str(exc))

    transaction.on_commit(_send)


def enqueue_chat_warm_after_commit(
    runner_id,
    *,
    chat_session_id,
    local_thread_id: str,
    local_session_id: str,
    cwd: str,
    model: str,
) -> None:
    def _send():
        try:
            send_to_runner(
                runner_id,
                {
                    "type": "chat_warm",
                    "chat_session_id": str(chat_session_id),
                    "local_thread_id": local_thread_id or None,
                    "local_session_id": local_session_id or None,
                    "cwd": cwd or None,
                    "model": model or None,
                },
            )
        except Exception:
            logger.exception("chat warm dispatch failed for session %s", chat_session_id)
            mark_warm_dispatch_failed(chat_session_id)

    transaction.on_commit(_send)


def mark_warm_dispatch_failed(session_id) -> None:
    with transaction.atomic():
        session = (
            AgentChatSession.objects.select_for_update()
            .filter(pk=session_id, status=AgentChatSessionStatus.OPEN)
            .first()
        )
        if session is None:
            return
        append_event_locked(
            session,
            "chat_warm_failed",
            {"code": "dispatch_failed"},
        )


def mark_message_dispatch_failed(session_id, message_id, detail: str) -> None:
    with transaction.atomic():
        session = AgentChatSession.objects.select_for_update().filter(pk=session_id).first()
        if session is None:
            return
        was_active = bool(session.active_message_id or session.active_turn_id)
        message = finalize_active_messages_locked(session, AgentChatMessageStatus.FAILED, message_id=message_id)
        session.active_message_id = None
        session.active_turn_id = ""
        session.error = detail[:2000]
        update_fields = [
            "active_message_id",
            "active_turn_id",
            "error",
            "updated_at",
        ]
        if session.close_requested:
            session.status = AgentChatSessionStatus.CLOSED
            session.closed_at = timezone.now()
            update_fields.extend(["status", "closed_at"])
        session.save(update_fields=update_fields)
        append_event_locked(
            session,
            "chat_failed",
            {"code": "dispatch_failed", "detail": detail},
            message=message,
        )
        if session.status == AgentChatSessionStatus.CLOSED:
            append_event_locked(session, "chat_closed", {"reason": "close_requested"})
        if was_active:
            drain_tasks_after_chat_release(session)


def create_assistant_message_locked(
    session: AgentChatSession,
    *,
    local_turn_id: str = "",
    local_item_id: str = "",
    status: str = AgentChatMessageStatus.STREAMING,
) -> AgentChatMessage:
    return AgentChatMessage.objects.create(
        session=session,
        role=AgentChatMessageRole.ASSISTANT,
        status=status,
        local_turn_id=local_turn_id[:128],
        local_item_id=local_item_id[:128],
        seq=next_message_seq_locked(session),
    )


def active_assistant_message_locked(session: AgentChatSession) -> Optional[AgentChatMessage]:
    qs = AgentChatMessage.objects.filter(
        session=session,
        role=AgentChatMessageRole.ASSISTANT,
        status=AgentChatMessageStatus.STREAMING,
    )
    if session.active_turn_id:
        assistant = qs.filter(local_turn_id=session.active_turn_id).order_by("-created_at").first()
        if assistant is not None:
            return assistant
    return qs.order_by("-created_at").first()


def finalize_active_messages_locked(
    session: AgentChatSession,
    final_status: str,
    *,
    message_id=None,
) -> Optional[AgentChatMessage]:
    now = timezone.now()
    message = None
    active_message_id = message_id or session.active_message_id
    if active_message_id:
        message = AgentChatMessage.objects.filter(pk=active_message_id, session=session).first()
        if message is not None:
            message.status = final_status
            message.completed_at = now
            message.save(update_fields=["status", "completed_at"])

    assistant = active_assistant_message_locked(session)
    if assistant is not None:
        assistant.status = final_status
        assistant.completed_at = now
        assistant.save(update_fields=["status", "completed_at"])
    return assistant or message


def complete_active_turn_locked(
    session: AgentChatSession,
    *,
    final_status: str = AgentChatMessageStatus.COMPLETED,
    payload: Optional[dict[str, Any]] = None,
) -> None:
    now = timezone.now()
    was_active = bool(session.active_message_id or session.active_turn_id)
    if session.active_message_id:
        AgentChatMessage.objects.filter(pk=session.active_message_id).update(
            status=final_status,
            completed_at=now,
        )
    session.active_message_id = None
    session.active_turn_id = ""
    session.last_message_at = now
    if session.close_requested and final_status in {
        AgentChatMessageStatus.COMPLETED,
        AgentChatMessageStatus.CANCELLED,
        AgentChatMessageStatus.FAILED,
    }:
        session.status = AgentChatSessionStatus.CLOSED
        session.closed_at = now
    session.save(
        update_fields=[
            "active_message_id",
            "active_turn_id",
            "last_message_at",
            "status",
            "closed_at",
            "updated_at",
        ]
    )
    append_event_locked(
        session,
        "turn_completed",
        payload or {"status": final_status},
    )
    if session.status == AgentChatSessionStatus.CLOSED:
        append_event_locked(session, "chat_closed", {"reason": "close_requested"})
    if was_active:
        drain_tasks_after_chat_release(session)


def sweep_active_turns() -> int:
    cutoff = timezone.now() - timedelta(seconds=CHAT_ACTIVE_TIMEOUT_SECS)
    sessions = list(
        AgentChatSession.objects.filter(status=AgentChatSessionStatus.OPEN)
        .filter(Q(active_message_id__isnull=False) | ~Q(active_turn_id=""))
        .filter(Q(updated_at__lt=cutoff) | Q(runner__status=RunnerStatus.OFFLINE))
        .values_list("id", flat=True)
    )
    count = 0
    for session_id in sessions:
        with transaction.atomic():
            session = (
                AgentChatSession.objects.select_for_update()
                .filter(pk=session_id, status=AgentChatSessionStatus.OPEN)
                .first()
            )
            if session is None:
                continue
            was_active = bool(session.active_message_id or session.active_turn_id)
            message = finalize_active_messages_locked(session, AgentChatMessageStatus.FAILED)
            session.active_message_id = None
            session.active_turn_id = ""
            session.error = "active chat turn timed out"
            update_fields = [
                "active_message_id",
                "active_turn_id",
                "error",
                "updated_at",
            ]
            if session.close_requested:
                session.status = AgentChatSessionStatus.CLOSED
                session.closed_at = timezone.now()
                update_fields.extend(["status", "closed_at"])
            session.save(update_fields=update_fields)
            append_event_locked(
                session,
                "chat_failed",
                {"code": "active_turn_timeout"},
                message=message,
            )
            if session.status == AgentChatSessionStatus.CLOSED:
                append_event_locked(session, "chat_closed", {"reason": "close_requested"})
            if was_active:
                drain_tasks_after_chat_release(session)
            count += 1
    return count


def release_active_chats_for_runner(runner: Runner, detail: str) -> int:
    sessions = list(
        AgentChatSession.objects.filter(runner=runner, status=AgentChatSessionStatus.OPEN)
        .filter(Q(active_message_id__isnull=False) | ~Q(active_turn_id=""))
        .values_list("id", flat=True)
    )
    count = 0
    for session_id in sessions:
        with transaction.atomic():
            session = (
                AgentChatSession.objects.select_for_update()
                .filter(pk=session_id, status=AgentChatSessionStatus.OPEN)
                .first()
            )
            if session is None or not (session.active_message_id or session.active_turn_id):
                continue
            message = finalize_active_messages_locked(session, AgentChatMessageStatus.FAILED)
            session.active_message_id = None
            session.active_turn_id = ""
            session.error = detail[:2000]
            session.save(update_fields=["active_message_id", "active_turn_id", "error", "updated_at"])
            append_event_locked(
                session,
                "chat_failed",
                {"code": "runner_session_reopened", "detail": detail},
                message=message,
            )
            drain_tasks_after_chat_release(session)
            count += 1
    return count


def sweep_empty_sessions() -> int:
    cutoff = timezone.now() - timedelta(hours=24)
    ids = list(
        AgentChatSession.objects.filter(
            status=AgentChatSessionStatus.OPEN,
            created_at__lt=cutoff,
            messages__isnull=True,
        ).values_list("id", flat=True)
    )
    if not ids:
        return 0
    return AgentChatSession.objects.filter(id__in=ids).update(
        status=AgentChatSessionStatus.CLOSED,
        closed_at=timezone.now(),
    )
