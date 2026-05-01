# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""HTTP endpoints for runner-upstream lifecycle + event POSTs.

See ``.ai_design/move_to_https/design.md`` §7.5. Each endpoint mirrors
a former ``ClientMsg`` variant. All require ``RunnerAccessTokenAuthentication``
plus ``run.runner_id == request.auth_runner.id``.

``Idempotency-Key`` is honored via ``RunMessageDedupe(run, message_id)``;
a duplicate POST returns 200 with no side effects.
"""

from __future__ import annotations

import json
import logging
from datetime import timedelta
from typing import Any, Callable, Dict, Optional

from django.db import IntegrityError, transaction
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.runner.authentication import (
    RunnerAccessTokenAuthentication,
    resolve_runner_for_run,
)
from pi_dash.runner.models import (
    AgentRun,
    AgentRunEvent,
    AgentRunStatus,
    ApprovalKind,
    ApprovalRequest,
    ApprovalStatus,
    Runner,
    RunMessageDedupe,
)

logger = logging.getLogger(__name__)

MAX_EVENT_PAYLOAD_BYTES = 64 * 1024


def _idempotency_key(request) -> str:
    return (request.headers.get("Idempotency-Key") or "").strip()


def _record_dedupe(run: AgentRun, message_id: str) -> bool:
    """Return True when the call is fresh; False when it's a duplicate."""
    if not message_id:
        return True
    try:
        with transaction.atomic():
            RunMessageDedupe.objects.create(run=run, message_id=message_id[:128])
        return True
    except IntegrityError:
        return False


class _RunEndpointBase(APIView):
    """Shared resolve / authorize for ``/runs/<run_id>/...`` endpoints."""

    authentication_classes = [RunnerAccessTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def _resolve(self, request, run_id) -> tuple[Optional[AgentRun], Optional[Response]]:
        run = (
            AgentRun.objects.select_related("work_item", "scheduler_binding")
            .filter(id=run_id)
            .first()
        )
        if run is None:
            return None, Response(
                {"error": "run_not_found"}, status=status.HTTP_404_NOT_FOUND
            )
        if not resolve_runner_for_run(run, request):
            return None, Response(
                {"error": "run_not_owned_by_runner"},
                status=status.HTTP_403_FORBIDDEN,
            )
        return run, None


def _drain_after_terminal(
    run_id, runner_id, pod_id, *, scheduler_binding_id=None
) -> None:
    from pi_dash.orchestration.scheduling import maybe_apply_deferred_pause
    from pi_dash.runner.services.matcher import (
        drain_for_runner_by_id,
        drain_pod_by_id,
    )

    run = (
        AgentRun.objects.select_related(
            "work_item", "work_item__state", "work_item__project", "scheduler_binding"
        )
        .filter(pk=run_id)
        .first()
    )
    if run is not None:
        try:
            maybe_apply_deferred_pause(run)
        except Exception:
            logger.exception("deferred-pause failed for run %s", run_id)
        if scheduler_binding_id is not None:
            try:
                from pi_dash.runner.services.scheduler_hook import (
                    update_scheduler_binding_on_terminate,
                )

                update_scheduler_binding_on_terminate(run)
            except Exception:
                logger.exception(
                    "scheduler binding update failed for run %s", run_id
                )
    drain_for_runner_by_id(runner_id)
    if pod_id is not None:
        drain_pod_by_id(pod_id)


class RunAcceptEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        if not _record_dedupe(run, _idempotency_key(request)):
            return Response({"ok": True, "duplicate": True})
        AgentRun.objects.filter(pk=run.pk).update(status=AgentRunStatus.RUNNING)
        return Response({"ok": True})


class RunStartedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        if not _record_dedupe(run, _idempotency_key(request)):
            return Response({"ok": True, "duplicate": True})
        thread_id = (request.data.get("thread_id") or "")[:128]
        AgentRun.objects.filter(pk=run.pk).update(
            status=AgentRunStatus.RUNNING,
            thread_id=thread_id,
            started_at=timezone.now(),
        )
        return Response({"ok": True})


class RunEventEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        # RunEvent batching is independent of lifecycle ordering
        # (design.md §7.5). Each event in the batch is keyed by seq;
        # the dedupe key is per-batch.
        if not _record_dedupe(run, _idempotency_key(request)):
            return Response({"ok": True, "duplicate": True})
        events = request.data.get("events") or [request.data]
        accepted = 0
        for ev in events:
            seq = int(ev.get("seq") or 0)
            kind = (ev.get("kind") or "")[:64]
            payload = ev.get("payload") or {}
            if not kind:
                continue
            try:
                encoded = json.dumps(payload, default=str)
            except (TypeError, ValueError):
                encoded = ""
            if len(encoded.encode("utf-8")) > MAX_EVENT_PAYLOAD_BYTES:
                payload = {
                    "_truncated": True,
                    "original_size_bytes": len(encoded.encode("utf-8")),
                }
            AgentRunEvent.objects.update_or_create(
                agent_run=run,
                seq=seq,
                defaults={"kind": kind, "payload": payload},
            )
            accepted += 1
        return Response({"ok": True, "accepted": accepted})


class RunApprovalEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        if not _record_dedupe(run, _idempotency_key(request)):
            return Response({"ok": True, "duplicate": True})
        approval_id = request.data.get("approval_id")
        kind_raw = (request.data.get("kind") or "").lower()
        kind = {
            "command_execution": ApprovalKind.COMMAND_EXECUTION,
            "file_change": ApprovalKind.FILE_CHANGE,
            "network_access": ApprovalKind.NETWORK_ACCESS,
        }.get(kind_raw, ApprovalKind.OTHER)
        ApprovalRequest.objects.update_or_create(
            id=approval_id,
            defaults={
                "agent_run": run,
                "kind": kind,
                "payload": request.data.get("payload") or {},
                "reason": request.data.get("reason") or "",
                "status": ApprovalStatus.PENDING,
                "expires_at": request.data.get("expires_at"),
            },
        )
        AgentRun.objects.filter(pk=run.pk).update(
            status=AgentRunStatus.AWAITING_APPROVAL
        )
        return Response({"ok": True})


class RunAwaitingReauthEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        if not _record_dedupe(run, _idempotency_key(request)):
            return Response({"ok": True, "duplicate": True})
        AgentRun.objects.filter(pk=run.pk).update(
            status=AgentRunStatus.AWAITING_REAUTH
        )
        return Response({"ok": True})


class RunCompletedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        if not _record_dedupe(run, _idempotency_key(request)):
            return Response({"ok": True, "duplicate": True})
        AgentRun.objects.filter(pk=run.pk).update(
            status=AgentRunStatus.COMPLETED,
            ended_at=timezone.now(),
            done_payload=request.data.get("done_payload"),
        )
        runner = getattr(request, "auth_runner", None)
        runner_id = runner.id if runner else None
        scheduler_binding_id = run.scheduler_binding_id
        transaction.on_commit(
            lambda rid=run.pk, rnr=runner_id, pid=run.pod_id, sb=scheduler_binding_id: _drain_after_terminal(
                rid, rnr, pid, scheduler_binding_id=sb
            )
        )
        return Response({"ok": True})


class RunPausedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        if not _record_dedupe(run, _idempotency_key(request)):
            return Response({"ok": True, "duplicate": True})
        payload = request.data.get("payload") or {}
        AgentRun.objects.filter(pk=run.pk).update(
            status=AgentRunStatus.PAUSED_AWAITING_INPUT,
            done_payload=payload,
        )
        return Response({"ok": True})


class RunFailedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        if not _record_dedupe(run, _idempotency_key(request)):
            return Response({"ok": True, "duplicate": True})
        AgentRun.objects.filter(pk=run.pk).update(
            status=AgentRunStatus.FAILED,
            ended_at=timezone.now(),
            error=(request.data.get("detail") or "")[:16000],
        )
        runner = getattr(request, "auth_runner", None)
        runner_id = runner.id if runner else None
        scheduler_binding_id = run.scheduler_binding_id
        transaction.on_commit(
            lambda rid=run.pk, rnr=runner_id, pid=run.pod_id, sb=scheduler_binding_id: _drain_after_terminal(
                rid, rnr, pid, scheduler_binding_id=sb
            )
        )
        return Response({"ok": True})


class RunCancelledEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        if not _record_dedupe(run, _idempotency_key(request)):
            return Response({"ok": True, "duplicate": True})
        AgentRun.objects.filter(pk=run.pk).update(
            status=AgentRunStatus.CANCELLED, ended_at=timezone.now()
        )
        runner = getattr(request, "auth_runner", None)
        runner_id = runner.id if runner else None
        scheduler_binding_id = run.scheduler_binding_id
        transaction.on_commit(
            lambda rid=run.pk, rnr=runner_id, pid=run.pod_id, sb=scheduler_binding_id: _drain_after_terminal(
                rid, rnr, pid, scheduler_binding_id=sb
            )
        )
        return Response({"ok": True})


class RunResumedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        if not _record_dedupe(run, _idempotency_key(request)):
            return Response({"ok": True, "duplicate": True})
        AgentRun.objects.filter(pk=run.pk).update(status=AgentRunStatus.RUNNING)
        return Response({"ok": True})


class RunStreamUpgradeEndpoint(_RunEndpointBase):
    """``POST /runs/<run_id>/stream/upgrade/`` — mint a 60s WS upgrade ticket.

    See ``design.md`` §7.9. v1 ships the endpoint and ticket store but
    no live consumer (deferred to first real use case).
    """

    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        stream = (request.data.get("stream") or "events").lower()
        if stream not in ("log", "events"):
            return Response(
                {"error": "invalid_stream"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        runner = getattr(request, "auth_runner", None)
        ticket = ""
        try:
            from pi_dash.settings.redis import redis_instance
            import uuid as _uuid

            client = redis_instance()
            if client is not None:
                ticket = _uuid.uuid4().hex
                client.set(
                    f"ws_upgrade_ticket:{ticket}",
                    json.dumps(
                        {
                            "run_id": str(run.id),
                            "stream": stream,
                            "runner_id": str(runner.id) if runner else "",
                            "expires_at": (
                                timezone.now() + timedelta(seconds=60)
                            ).isoformat(),
                        }
                    ),
                    ex=60,
                )
        except Exception:
            logger.exception("failed to mint ws upgrade ticket")
        return Response({"ticket": ticket, "expires_in_secs": 60})
