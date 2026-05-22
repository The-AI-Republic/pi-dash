# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import asyncio
import json
import time
from types import SimpleNamespace
from typing import Optional

import redis.asyncio as aioredis
from django.conf import settings
from django.db import transaction
from django.db.models import Q
from django.http import JsonResponse, StreamingHttpResponse
from django.utils import timezone
from rest_framework import status
from rest_framework.exceptions import AuthenticationFailed
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.throttling import UserRateThrottle
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.authentication import RunnerAccessTokenAuthentication
from pi_dash.runner.models import (
    AgentChatApprovalRequest,
    AgentChatEvent,
    AgentChatMessage,
    AgentChatMessageRole,
    AgentChatMessageStatus,
    AgentChatSession,
    AgentChatSessionStatus,
    ApprovalKind,
    ApprovalStatus,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.serializers import (
    AgentChatApprovalRequestSerializer,
    AgentChatEventSerializer,
    AgentChatMessageSerializer,
    AgentChatSessionSerializer,
    ApprovalDecisionSerializer,
)
from pi_dash.runner.services import chat as chat_service
from pi_dash.runner.services.permissions import (
    is_workspace_admin,
    is_workspace_member,
)
from pi_dash.runner.services.pubsub import send_to_runner


class ChatSendThrottle(UserRateThrottle):
    scope = "runner_chat_send"


CHAT_EVENT_PAYLOAD_MAX_BYTES = 256 * 1024
_async_redis_client: Optional[aioredis.Redis] = None


def _idempotency_key(request) -> str:
    return (request.headers.get("Idempotency-Key") or "").strip()


def _missing_idempotency_key_response() -> Response:
    return Response({"error": "idempotency_key_required"}, status=status.HTTP_400_BAD_REQUEST)


def _payload_too_large(value) -> bool:
    try:
        return len(json.dumps(value, default=str).encode("utf-8")) > CHAT_EVENT_PAYLOAD_MAX_BYTES
    except (TypeError, ValueError):
        return True


def _assistant_delta_text(payload) -> str:
    if not isinstance(payload, dict):
        return ""
    params = payload.get("params")
    if not isinstance(params, dict):
        return ""
    delta = params.get("delta")
    if isinstance(delta, str):
        return delta
    if isinstance(delta, dict):
        text = delta.get("text")
        if isinstance(text, str):
            return text
    text = params.get("text")
    return text if isinstance(text, str) else ""


def _async_redis() -> aioredis.Redis:
    global _async_redis_client
    if _async_redis_client is None:
        redis_url = getattr(settings, "REDIS_URL", "") or "redis://localhost:6379/0"
        _async_redis_client = aioredis.from_url(redis_url, decode_responses=True)
    return _async_redis_client


def _session_authenticates(request) -> bool:
    try:
        auth_result = BaseSessionAuthentication().authenticate(SimpleNamespace(_request=request))
    except AuthenticationFailed:
        return False
    if auth_result is not None:
        request.user = auth_result[0]
    return bool(getattr(request, "user", None) and request.user.is_authenticated)


def _runner_unavailable(runner: Runner) -> bool:
    return runner.status in {RunnerStatus.OFFLINE, RunnerStatus.REVOKED}


def _approval_kind(raw: str) -> str:
    return {
        "command_execution": ApprovalKind.COMMAND_EXECUTION,
        "file_change": ApprovalKind.FILE_CHANGE,
        "network_access": ApprovalKind.NETWORK_ACCESS,
    }.get((raw or "").lower(), ApprovalKind.OTHER)


class AgentChatSessionListEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request):
        workspace_id = request.query_params.get("workspace")
        runner_id = request.query_params.get("runner")
        qs = AgentChatSession.objects.select_related("runner", "pod")
        if workspace_id:
            if not is_workspace_member(request.user, workspace_id):
                return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
            qs = qs.filter(workspace_id=workspace_id)
        if runner_id:
            qs = qs.filter(runner_id=runner_id)
        if not workspace_id or not is_workspace_admin(request.user, workspace_id):
            qs = qs.filter(created_by=request.user)
        qs = qs.order_by("-last_message_at", "-created_at")[:100]
        return Response(AgentChatSessionSerializer(qs, many=True).data)

    def post(self, request):
        workspace_id = request.data.get("workspace")
        runner_id = request.data.get("runner")
        if not workspace_id or not runner_id:
            return Response(
                {"error": "workspace and runner are required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not is_workspace_member(request.user, workspace_id):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        with transaction.atomic():
            runner = (
                Runner.objects.select_for_update()
                .filter(pk=runner_id, workspace_id=workspace_id)
                .first()
            )
            if runner is None:
                return Response({"error": "runner_not_found"}, status=status.HTTP_404_NOT_FOUND)
            if _runner_unavailable(runner):
                return Response(
                    {"error": "runner_unavailable"},
                    status=status.HTTP_409_CONFLICT,
                )
            existing = (
                AgentChatSession.objects.filter(
                    created_by=request.user,
                    runner=runner,
                    status=AgentChatSessionStatus.OPEN,
                    messages__isnull=True,
                )
                .order_by("-created_at")
                .first()
            )
            if existing is not None:
                return Response(AgentChatSessionSerializer(existing).data)
            session = AgentChatSession.objects.create(
                workspace_id=workspace_id,
                runner=runner,
                created_by=request.user,
                pod=runner.pod,
                model=(request.data.get("model") or "")[:128],
                cwd=chat_service.normalize_cwd(request.data.get("cwd")),
            )
        return Response(
            AgentChatSessionSerializer(session).data,
            status=status.HTTP_201_CREATED,
        )


class AgentChatSessionDetailEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request, session_id):
        session = AgentChatSession.objects.select_related("runner", "pod").filter(pk=session_id).first()
        if session is None or not chat_service.can_read_chat(request.user, session):
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        return Response(AgentChatSessionSerializer(session).data)


class AgentChatWarmEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, session_id):
        with transaction.atomic():
            session = (
                AgentChatSession.objects.select_for_update()
                .select_related("runner")
                .filter(pk=session_id)
                .first()
            )
            if session is None or not chat_service.can_send_chat(request.user, session):
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
            runner = (
                Runner.objects.select_for_update()
                .filter(pk=session.runner_id)
                .first()
            )
            if session.status != AgentChatSessionStatus.OPEN:
                return Response(
                    {"error": "chat_session_closed"},
                    status=status.HTTP_409_CONFLICT,
                )
            if runner is None or _runner_unavailable(runner):
                return Response(
                    {"error": "runner_unavailable"},
                    status=status.HTTP_409_CONFLICT,
                )
            if session.active_message_id is not None or session.active_turn_id:
                return Response({"ok": True, "skipped": "chat_turn_active"})
            if chat_service.runner_has_active_task(runner) or runner.status == RunnerStatus.BUSY:
                return Response(
                    {"error": "runner_busy"}, status=status.HTTP_409_CONFLICT
                )
            chat_service.enqueue_chat_warm_after_commit(
                runner.id,
                chat_session_id=session.id,
                local_thread_id=session.local_thread_id,
                local_session_id=session.local_session_id,
                cwd=session.cwd,
                model=session.model,
            )
        return Response({"ok": True}, status=status.HTTP_202_ACCEPTED)


class AgentChatMessageListEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]
    throttle_classes = [ChatSendThrottle]

    def get_throttles(self):
        if self.request.method == "POST":
            return super().get_throttles()
        return []

    def get(self, request, session_id):
        session = AgentChatSession.objects.filter(pk=session_id).first()
        if session is None or not chat_service.can_read_chat(request.user, session):
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        qs = AgentChatMessage.objects.filter(session=session).order_by("seq")
        return Response(AgentChatMessageSerializer(qs, many=True).data)

    def post(self, request, session_id):
        content = request.data.get("content") or ""
        content_parts = request.data.get("content_parts") or []
        if not content and not content_parts:
            return Response(
                {"error": "content is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        with transaction.atomic():
            session = (
                AgentChatSession.objects.select_for_update()
                .select_related("runner")
                .filter(pk=session_id)
                .first()
            )
            if session is None or not chat_service.can_send_chat(request.user, session):
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
            runner = (
                Runner.objects.select_for_update()
                .filter(pk=session.runner_id)
                .first()
            )
            if session.status != AgentChatSessionStatus.OPEN:
                return Response(
                    {"error": "chat_session_closed"},
                    status=status.HTTP_409_CONFLICT,
                )
            if runner is None or _runner_unavailable(runner):
                return Response(
                    {"error": "runner_unavailable"},
                    status=status.HTTP_409_CONFLICT,
                )
            if chat_service.runner_has_active_task(runner) or runner.status == RunnerStatus.BUSY:
                return Response(
                    {"error": "runner_busy"}, status=status.HTTP_409_CONFLICT
                )
            if session.active_message_id is not None or session.active_turn_id:
                return Response(
                    {"error": "chat_turn_active"},
                    status=status.HTTP_409_CONFLICT,
                )
            message = AgentChatMessage.objects.create(
                session=session,
                role=AgentChatMessageRole.USER,
                content=content,
                content_parts=content_parts,
                status=AgentChatMessageStatus.QUEUED,
                seq=chat_service.next_message_seq_locked(session),
            )
            session.active_message_id = message.id
            session.last_message_at = timezone.now()
            session.save(
                update_fields=["active_message_id", "last_message_at", "updated_at"]
            )
            chat_service.enqueue_chat_message_after_commit(
                runner.id,
                chat_session_id=session.id,
                message_id=message.id,
                content=content,
                content_parts=content_parts,
                local_thread_id=session.local_thread_id,
                local_session_id=session.local_session_id,
                cwd=session.cwd,
                model=session.model,
            )
        return Response(AgentChatMessageSerializer(message).data, status=status.HTTP_201_CREATED)


class AgentChatCancelEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, session_id):
        with transaction.atomic():
            session = (
                AgentChatSession.objects.select_for_update()
                .select_related("runner")
                .filter(pk=session_id)
                .first()
            )
            if session is None or not chat_service.can_send_chat(request.user, session):
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
            if not session.active_message_id and not session.active_turn_id:
                return Response({"ok": True, "noop": True})
            runner_id = session.runner_id
            transaction.on_commit(
                lambda: send_to_runner(
                    runner_id,
                    {
                        "type": "chat_cancel",
                        "chat_session_id": str(session.id),
                        "reason": request.data.get("reason") or "user_cancelled",
                    },
                )
            )
        return Response({"ok": True})


class AgentChatCloseEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, session_id):
        with transaction.atomic():
            session = (
                AgentChatSession.objects.select_for_update()
                .select_related("runner")
                .filter(pk=session_id)
                .first()
            )
            if session is None or not chat_service.can_send_chat(request.user, session):
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
            if session.active_message_id or session.active_turn_id:
                session.close_requested = True
                session.save(update_fields=["close_requested", "updated_at"])
                runner_id = session.runner_id
                transaction.on_commit(
                    lambda: send_to_runner(
                        runner_id,
                        {
                            "type": "chat_cancel",
                            "chat_session_id": str(session.id),
                            "reason": "close_requested",
                        },
                    )
                )
                return Response(AgentChatSessionSerializer(session).data)
            session.status = AgentChatSessionStatus.CLOSED
            session.closed_at = timezone.now()
            session.save(update_fields=["status", "closed_at", "updated_at"])
            chat_service.append_event_locked(
                session,
                "chat_closed",
                {"reason": request.data.get("reason") or "user_closed"},
            )
            runner_id = session.runner_id
            transaction.on_commit(
                lambda: send_to_runner(
                    runner_id,
                    {
                        "type": "chat_close",
                        "chat_session_id": str(session.id),
                        "reason": request.data.get("reason") or "user_closed",
                    },
                )
            )
        return Response(AgentChatSessionSerializer(session).data)


class AgentChatApprovalListEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request):
        workspace_id = request.query_params.get("workspace")
        qs = AgentChatApprovalRequest.objects.select_related("session").filter(
            status=ApprovalStatus.PENDING,
        )
        if workspace_id:
            if not is_workspace_member(request.user, workspace_id):
                return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
            qs = qs.filter(session__workspace_id=workspace_id)
            if not is_workspace_admin(request.user, workspace_id):
                qs = qs.filter(session__created_by=request.user)
        else:
            qs = qs.filter(
                Q(session__created_by=request.user)
                | Q(
                    session__workspace__workspace_member__member=request.user,
                    session__workspace__workspace_member__role__gte=20,
                    session__workspace__workspace_member__deleted_at__isnull=True,
                )
            ).distinct()
        return Response(AgentChatApprovalRequestSerializer(qs[:200], many=True).data)


class AgentChatApprovalDecideEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, approval_id):
        serializer = ApprovalDecisionSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        decision = serializer.validated_data["decision"]
        with transaction.atomic():
            approval = (
                AgentChatApprovalRequest.objects.select_for_update()
                .select_related("session")
                .filter(pk=approval_id)
                .first()
            )
            if approval is None or not chat_service.can_decide_chat_approval(request.user, approval):
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
            if approval.status != ApprovalStatus.PENDING:
                return Response(
                    {"error": "already decided"},
                    status=status.HTTP_409_CONFLICT,
                )
            approval.status = (
                ApprovalStatus.ACCEPTED
                if decision == "accept"
                else ApprovalStatus.DECLINED
            )
            approval.decision_source = "web"
            approval.decided_by = request.user
            approval.decided_at = timezone.now()
            approval.save(
                update_fields=[
                    "status",
                    "decision_source",
                    "decided_by",
                    "decided_at",
                ]
            )
            chat_service.append_event_locked(
                approval.session,
                "approval_decided",
                {"approval_id": str(approval.id), "decision": decision},
            )
            runner_id = approval.session.runner_id
            transaction.on_commit(
                lambda: send_to_runner(
                    runner_id,
                    {
                        "type": "chat_decide",
                        "chat_session_id": str(approval.session_id),
                        "approval_id": str(approval.id),
                        "local_approval_id": approval.local_approval_id,
                        "decision": decision,
                        "decided_by": str(request.user.id),
                    },
                )
            )
        return Response(AgentChatApprovalRequestSerializer(approval).data)


class _ChatRunnerEndpointBase(APIView):
    authentication_classes = [RunnerAccessTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def _resolve(self, request, session_id) -> tuple[Optional[AgentChatSession], Optional[Response]]:
        session = AgentChatSession.objects.filter(pk=session_id).first()
        if session is None:
            return None, Response({"error": "chat_session_not_found"}, status=status.HTTP_404_NOT_FOUND)
        if session.runner_id != getattr(request, "auth_runner", None).id:
            return None, Response({"error": "chat_session_not_owned_by_runner"}, status=status.HTTP_403_FORBIDDEN)
        return session, None


class ChatStartedEndpoint(_ChatRunnerEndpointBase):
    def post(self, request, session_id):
        session, err = self._resolve(request, session_id)
        if err:
            return err
        key = _idempotency_key(request)
        if not key:
            return _missing_idempotency_key_response()
        with transaction.atomic():
            session = AgentChatSession.objects.select_for_update().get(pk=session.pk)
            if not chat_service.record_dedupe(session, key):
                return Response({"ok": True, "duplicate": True})
            session.local_thread_id = (request.data.get("local_thread_id") or "")[:128]
            session.local_session_id = (request.data.get("local_session_id") or "")[:128]
            session.agent_kind = (request.data.get("agent_kind") or "")[:24]
            session.save(update_fields=["local_thread_id", "local_session_id", "agent_kind", "updated_at"])
            chat_service.append_event_locked(
                session,
                "chat_started",
                {
                    "local_thread_id": session.local_thread_id,
                    "local_session_id": session.local_session_id,
                    "agent_kind": session.agent_kind,
                },
            )
        return Response({"ok": True})


class ChatMessageStartedEndpoint(_ChatRunnerEndpointBase):
    def post(self, request, session_id, message_id):
        session, err = self._resolve(request, session_id)
        if err:
            return err
        source_key = f"message_started:{message_id}"
        with transaction.atomic():
            session = AgentChatSession.objects.select_for_update().get(pk=session.pk)
            if AgentChatEvent.objects.filter(session=session, source_key=source_key).exists():
                return Response({"ok": True, "duplicate": True})
            turn_id = (request.data.get("turn_id") or "")[:128]
            AgentChatMessage.objects.filter(pk=message_id, session=session).update(
                status=AgentChatMessageStatus.SENT,
                local_turn_id=turn_id,
            )
            session.active_turn_id = turn_id
            session.save(update_fields=["active_turn_id", "updated_at"])
            chat_service.append_event_locked(
                session,
                "turn_started",
                {"message_id": str(message_id), "turn_id": turn_id},
                source_key=source_key,
            )
        return Response({"ok": True})


class ChatEventEndpoint(_ChatRunnerEndpointBase):
    def post(self, request, session_id):
        session, err = self._resolve(request, session_id)
        if err:
            return err
        key = _idempotency_key(request)
        if not key:
            return _missing_idempotency_key_response()
        with transaction.atomic():
            session = AgentChatSession.objects.select_for_update().get(pk=session.pk)
            existing = AgentChatEvent.objects.filter(session=session, source_key=key[:160]).first()
            if existing is not None:
                return Response({"ok": True, "duplicate": True, "event": AgentChatEventSerializer(existing).data})
            kind = (request.data.get("kind") or "raw")[:64]
            payload = request.data.get("payload") or {}
            bridge_seq = request.data.get("bridge_seq")
            if bridge_seq is not None:
                payload = dict(payload) if isinstance(payload, dict) else {"value": payload}
                payload["bridge_seq"] = bridge_seq
            if _payload_too_large(payload):
                return Response({"error": "payload_too_large"}, status=status.HTTP_413_REQUEST_ENTITY_TOO_LARGE)
            event = chat_service.append_event_locked(
                session,
                kind,
                payload,
                source_key=key,
            )
            if kind == "assistant_delta":
                assistant = (
                    AgentChatMessage.objects.filter(
                        session=session,
                        role=AgentChatMessageRole.ASSISTANT,
                        status=AgentChatMessageStatus.STREAMING,
                    )
                    .order_by("-created_at")
                    .first()
                )
                if assistant is None:
                    assistant = chat_service.create_assistant_message_locked(
                        session, local_turn_id=session.active_turn_id
                    )
                delta_text = _assistant_delta_text(payload)
                if delta_text:
                    assistant.content = f"{assistant.content or ''}{delta_text}"
                    assistant.save(update_fields=["content"])
                event.message = assistant
                event.save(update_fields=["message"])
        return Response({"ok": True, "event": AgentChatEventSerializer(event).data})


class ChatApprovalEndpoint(_ChatRunnerEndpointBase):
    def post(self, request, session_id):
        session, err = self._resolve(request, session_id)
        if err:
            return err
        local_approval_id = (request.data.get("local_approval_id") or "")[:160]
        if not local_approval_id:
            return Response({"error": "local_approval_id_required"}, status=status.HTTP_400_BAD_REQUEST)
        payload = request.data.get("payload") or {}
        if _payload_too_large(payload):
            return Response({"error": "payload_too_large"}, status=status.HTTP_413_REQUEST_ENTITY_TOO_LARGE)
        with transaction.atomic():
            session = AgentChatSession.objects.select_for_update().get(pk=session.pk)
            approval, _ = AgentChatApprovalRequest.objects.update_or_create(
                session=session,
                local_approval_id=local_approval_id,
                defaults={
                    "kind": _approval_kind(request.data.get("kind") or ""),
                    "payload": payload,
                    "reason": request.data.get("reason") or "",
                    "status": ApprovalStatus.PENDING,
                    "expires_at": request.data.get("expires_at"),
                },
            )
            chat_service.append_event_locked(
                session,
                "approval_requested",
                {"approval_id": str(approval.id), "local_approval_id": local_approval_id},
                source_key=f"approval_requested:{local_approval_id}",
            )
        return Response({"ok": True, "approval": AgentChatApprovalRequestSerializer(approval).data})


class ChatMessageCompleteEndpoint(_ChatRunnerEndpointBase):
    def post(self, request, session_id, message_id):
        session, err = self._resolve(request, session_id)
        if err:
            return err
        source_key = f"message_complete:{message_id}"
        with transaction.atomic():
            session = AgentChatSession.objects.select_for_update().get(pk=session.pk)
            if AgentChatEvent.objects.filter(session=session, source_key=source_key).exists():
                return Response({"ok": True, "duplicate": True})
            final = request.data.get("status") or AgentChatMessageStatus.COMPLETED
            if final not in {
                AgentChatMessageStatus.COMPLETED,
                AgentChatMessageStatus.CANCELLED,
                AgentChatMessageStatus.FAILED,
            }:
                final = AgentChatMessageStatus.COMPLETED
            assistant_text = request.data.get("assistant_message") or ""
            assistant = (
                AgentChatMessage.objects.filter(
                    session=session,
                    role=AgentChatMessageRole.ASSISTANT,
                    status=AgentChatMessageStatus.STREAMING,
                )
                .order_by("-created_at")
                .first()
            )
            if assistant_text or assistant is not None:
                if assistant is None:
                    assistant = chat_service.create_assistant_message_locked(
                        session, local_turn_id=request.data.get("turn_id") or ""
                    )
                update_fields = ["status", "completed_at"]
                if assistant_text:
                    assistant.content = assistant_text
                    update_fields.append("content")
                assistant.status = final
                assistant.completed_at = timezone.now()
                assistant.save(update_fields=update_fields)
            chat_service.complete_active_turn_locked(
                session,
                final_status=final,
                payload={"status": final, "message_id": str(message_id)},
            )
            chat_service.append_event_locked(
                session,
                "raw",
                {"kind": "message_complete"},
                source_key=source_key,
            )
        return Response({"ok": True})


class ChatFailedEndpoint(_ChatRunnerEndpointBase):
    def post(self, request, session_id):
        session, err = self._resolve(request, session_id)
        if err:
            return err
        key = _idempotency_key(request)
        if not key:
            return _missing_idempotency_key_response()
        with transaction.atomic():
            session = AgentChatSession.objects.select_for_update().get(pk=session.pk)
            if not chat_service.record_dedupe(session, key):
                return Response({"ok": True, "duplicate": True})
            code = request.data.get("code") or "chat_failed"
            detail = request.data.get("detail") or ""
            if session.active_message_id:
                AgentChatMessage.objects.filter(pk=session.active_message_id).update(
                    status=AgentChatMessageStatus.FAILED,
                    completed_at=timezone.now(),
                )
            should_close = session.close_requested
            session.active_message_id = None
            session.active_turn_id = ""
            session.error = detail[:2000]
            update_fields = ["active_message_id", "active_turn_id", "error", "updated_at"]
            if should_close:
                session.status = AgentChatSessionStatus.CLOSED
                session.closed_at = timezone.now()
                update_fields.extend(["status", "closed_at"])
            session.save(update_fields=update_fields)
            chat_service.append_event_locked(session, "chat_failed", {"code": code, "detail": detail})
            if should_close:
                chat_service.append_event_locked(session, "chat_closed", {"reason": "close_requested"})
        return Response({"ok": True})


class ChatClosedEndpoint(_ChatRunnerEndpointBase):
    def post(self, request, session_id):
        session, err = self._resolve(request, session_id)
        if err:
            return err
        key = _idempotency_key(request)
        if not key:
            return _missing_idempotency_key_response()
        with transaction.atomic():
            session = AgentChatSession.objects.select_for_update().get(pk=session.pk)
            if not chat_service.record_dedupe(session, key):
                return Response({"ok": True, "duplicate": True})
            session.status = AgentChatSessionStatus.CLOSED
            session.active_message_id = None
            session.active_turn_id = ""
            session.closed_at = timezone.now()
            session.save(update_fields=["status", "active_message_id", "active_turn_id", "closed_at", "updated_at"])
            chat_service.append_event_locked(
                session,
                "chat_closed",
                {"reason": request.data.get("reason") or "runner_closed"},
            )
        return Response({"ok": True})


async def chat_event_stream(request, session_id):
    if not await asyncio.to_thread(_session_authenticates, request):
        return JsonResponse(
            {"error": "authentication required"},
            status=status.HTTP_403_FORBIDDEN,
        )
    session = await AgentChatSession.objects.filter(pk=session_id).afirst()
    if session is None or not await asyncio.to_thread(chat_service.can_read_chat, request.user, session):
        return JsonResponse({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)

    after_raw = request.GET.get("after") or request.headers.get("Last-Event-ID") or "0"
    try:
        after = int(after_raw)
    except ValueError:
        after = 0

    async def _events():
        client = _async_redis()
        pubsub = client.pubsub(ignore_subscribe_messages=True)
        last_seq = after
        last_heartbeat = time.monotonic()
        try:
            await pubsub.subscribe(chat_service.event_channel(session_id))
            async for event in AgentChatEvent.objects.filter(session_id=session_id, seq__gt=after).order_by("seq"):
                data = chat_service.serialize_event(event)
                last_seq = max(last_seq, event.seq)
                yield f"event: chat.event\nid: {event.seq}\ndata: {json.dumps(data, default=str)}\n\n"
            while True:
                msg = await pubsub.get_message(timeout=1.0)
                if msg and msg.get("type") == "message":
                    data = json.loads(msg.get("data") or "{}")
                    seq = int(data.get("seq") or 0)
                    if seq > last_seq:
                        last_seq = seq
                        yield f"event: chat.event\nid: {seq}\ndata: {json.dumps(data, default=str)}\n\n"
                if time.monotonic() - last_heartbeat >= 15:
                    last_heartbeat = time.monotonic()
                    yield ": heartbeat\n\n"
        finally:
            await pubsub.unsubscribe(chat_service.event_channel(session_id))
            await pubsub.close()

    return StreamingHttpResponse(_events(), content_type="text/event-stream")
