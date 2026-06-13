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
from typing import Optional

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
    RunMessageDedupe,
)
from pi_dash.runner.services import run_lifecycle

logger = logging.getLogger(__name__)

MAX_EVENT_PAYLOAD_BYTES = 64 * 1024


def _idempotency_key(request) -> str:
    return (request.headers.get("Idempotency-Key") or "").strip()


def _record_dedupe(run: AgentRun, message_id: str) -> bool:
    """Return True when the call is fresh; False when it's a duplicate.

    Caller MUST invoke this inside an outer ``transaction.atomic()``
    block that also contains the side effect being deduped — otherwise
    a side-effect failure (DB blip, deadlock, scheduler hook error)
    after the dedupe row commits will leave the dedupe in place but
    the state transition unapplied, and the runner's retry will get
    ``{"ok": True, "duplicate": True}`` and silently drop the
    transition. The inner ``transaction.atomic()`` here becomes a
    savepoint when nested; ``IntegrityError`` rolls back just the
    savepoint, keeping the outer transaction valid.
    """
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

    def _lock_non_terminal(
        self, run: AgentRun
    ) -> tuple[Optional[AgentRun], Optional[Response]]:
        locked = AgentRun.objects.select_for_update().filter(pk=run.pk).first()
        if locked is None:
            return None, Response(
                {"error": "run_not_found"}, status=status.HTTP_404_NOT_FOUND
            )
        if locked.status in run_lifecycle.TERMINAL_RUN_STATUSES:
            return locked, Response({"ok": True, "terminal": True})
        return locked, None


class RunAcceptEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            AgentRun.objects.filter(pk=locked.pk).update(
                status=AgentRunStatus.RUNNING,
                # The run left the daemon's local worktree queue — a stale
                # position must not linger on a now-running row.
                queue_position=None,
            )
        return Response({"ok": True})


class RunQueuedEndpoint(_RunEndpointBase):
    """``POST /runs/<run_id>/queued`` — runner reports the run is waiting.

    The runner accepted the run but cannot acquire a worktree lease yet, so it
    sits in the daemon's local queue. Body carries ``queue_position`` (the
    run's position in that queue). See
    ``.ai_design/worktree_pooling/design.md`` §6.1.

    Transition rules:

    - ``ASSIGNED`` → ``WAITING_FOR_WORKTREE`` (and store the position).
    - ``WAITING_FOR_WORKTREE`` → ``WAITING_FOR_WORKTREE`` (position refresh as
      the queue drains; positions only decrease).
    - ``RUNNING`` or any terminal status → acknowledged and dropped without a
      state change. A late or duplicate ``queued`` post must never regress a
      run that already started (or finished); the runner posts ``accept`` at
      lease grant, which is what actually drives ``RUNNING``.
    """

    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                # Terminal: acknowledge so the runner stops retrying; the
                # ``_lock_non_terminal`` helper already returns ``terminal``.
                return closed
            if locked.status not in (
                AgentRunStatus.ASSIGNED,
                AgentRunStatus.WAITING_FOR_WORKTREE,
            ):
                # RUNNING (or any other non-terminal state): acknowledge and
                # drop. A run that has already started must not regress to
                # WAITING_FOR_WORKTREE on a late/duplicate queued post.
                return Response({"ok": True, "ignored": True})
            position = _parse_queue_position(request.data.get("queue_position"))
            AgentRun.objects.filter(pk=locked.pk).update(
                status=AgentRunStatus.WAITING_FOR_WORKTREE,
                queue_position=position,
            )
        return Response({"ok": True})


# ``AgentRun.queue_position`` is a PositiveSmallIntegerField; Postgres
# rejects anything above the signed-int16 ceiling, so out-of-range reports
# are clamped rather than allowed to 500 the endpoint.
QUEUE_POSITION_MAX = 32767


def _parse_queue_position(raw) -> Optional[int]:
    """Coerce a reported queue position to a non-negative int, else ``None``.

    A missing or malformed value clears the stored position rather than
    erroring — the field is display-only and never load-bearing. Values
    beyond the column's int16 range are clamped for the same reason.
    """
    if raw is None:
        return None
    try:
        value = int(raw)
    except (TypeError, ValueError):
        return None
    if value < 0:
        return None
    return min(value, QUEUE_POSITION_MAX)


class RunStartedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            thread_id = (request.data.get("thread_id") or "")[:128]
            updates = {
                "status": AgentRunStatus.RUNNING,
                "thread_id": thread_id,
                "started_at": timezone.now(),
            }
            model = str(request.data.get("model") or "").strip()
            if model:
                updates["llm_model"] = model[:128]
            AgentRun.objects.filter(pk=locked.pk).update(**updates)
        return Response({"ok": True})


class RunEventEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        # RunEvent batching is independent of lifecycle ordering
        # (design.md §7.5). Each event in the batch is keyed by seq;
        # the dedupe key is per-batch.
        if run.is_terminal:
            # A late-arriving batch after RunCompleted/RunFailed/
            # RunCancelled is not actionable: appending to a closed run
            # would surface as ghost activity in the UI and the
            # observability bridge. Acknowledge so the runner stops
            # retrying, but record nothing.
            return Response({"ok": True, "terminal": True, "accepted": 0})
        with transaction.atomic():
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
        with transaction.atomic():
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
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
                    "agent_run": locked,
                    "kind": kind,
                    "payload": request.data.get("payload") or {},
                    "reason": request.data.get("reason") or "",
                    "status": ApprovalStatus.PENDING,
                    "expires_at": request.data.get("expires_at"),
                },
            )
            AgentRun.objects.filter(pk=locked.pk).update(
                status=AgentRunStatus.AWAITING_APPROVAL
            )
        return Response({"ok": True})


class RunAwaitingReauthEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            AgentRun.objects.filter(pk=locked.pk).update(
                status=AgentRunStatus.AWAITING_REAUTH
            )
        return Response({"ok": True})


class RunCompletedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            runner = getattr(request, "auth_runner", None)
            if runner is not None:
                run_lifecycle.finalize_run_terminal(
                    runner,
                    run.id,
                    AgentRunStatus.COMPLETED,
                    done_payload=request.data.get("done_payload"),
                    tokens=request.data.get("tokens") or request.data.get("usage"),
                    model=request.data.get("model"),
                )
        return Response({"ok": True})


class RunPausedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            _, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            runner = getattr(request, "auth_runner", None)
            if runner is None:
                return Response({"ok": True})
            payload = request.data.get("payload") or {}
            # Posts the agent's question to the issue thread,
            # applies deferred-pause workspace transitions, and re-fires
            # drain. See run_lifecycle.apply_run_paused.
            run_lifecycle.apply_run_paused(
                runner,
                run.id,
                payload,
                tokens=request.data.get("tokens") or request.data.get("usage"),
                model=request.data.get("model"),
            )
        return Response({"ok": True})


class RunFailedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            runner = getattr(request, "auth_runner", None)
            if runner is None:
                return Response({"ok": True})
            # Resume-unavailable is a re-queue, not a terminal failure.
            # Without this branch, runs that miss their session on disk
            # fail-stop instead of falling back into the pod's queue with
            # a fresh session.
            if (request.data.get("reason") or "") == "resume_unavailable":
                locked, closed = self._lock_non_terminal(run)
                if closed:
                    return closed
                run_lifecycle.apply_run_resume_unavailable(
                    runner,
                    locked.id,
                    locked_run=locked,
                )
                return Response({"ok": True, "rescheduled": True})
            # The runner rejected this Assign because its agent is still
            # busy (the matcher freed it in DB terms before the local
            # agent actually stopped — e.g. right after a user cancel).
            # Re-queue for a fresh dispatch instead of fail-stopping;
            # without this branch a NACKed run would read as a crash.
            if (request.data.get("reason") or "") == "assign_rejected_busy":
                locked, closed = self._lock_non_terminal(run)
                if closed:
                    return closed
                run_lifecycle.apply_assign_rejected_busy(
                    runner,
                    locked.id,
                    locked_run=locked,
                )
                return Response({"ok": True, "rescheduled": True})
            # A safety-classifier decline (e.g. Claude Fable 5 cyber/bio) is a
            # terminal REFUSED, not a generic crash. The runner reports
            # `reason: "refusal"` with a `category`; record both so a policy
            # decline stays queryable apart from a FAILED.
            if (request.data.get("reason") or "") == "refusal":
                run_lifecycle.finalize_run_terminal(
                    runner,
                    run.id,
                    AgentRunStatus.REFUSED,
                    error_detail=request.data.get("detail") or "",
                    refusal_category=request.data.get("category"),
                    tokens=request.data.get("tokens") or request.data.get("usage"),
                    model=request.data.get("model"),
                )
                return Response({"ok": True, "refused": True})
            run_lifecycle.finalize_run_terminal(
                runner,
                run.id,
                AgentRunStatus.FAILED,
                error_detail=request.data.get("detail") or "",
                tokens=request.data.get("tokens") or request.data.get("usage"),
                model=request.data.get("model"),
            )
        return Response({"ok": True})


class RunCancelledEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            runner = getattr(request, "auth_runner", None)
            if runner is not None:
                run_lifecycle.finalize_run_terminal(
                    runner,
                    run.id,
                    AgentRunStatus.CANCELLED,
                    tokens=request.data.get("tokens") or request.data.get("usage"),
                    model=request.data.get("model"),
                )
        return Response({"ok": True})


class RunResumedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            AgentRun.objects.filter(pk=locked.pk).update(
                status=AgentRunStatus.RUNNING
            )
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
            if client is None:
                return Response(
                    {"error": "redis_unavailable"},
                    status=status.HTTP_503_SERVICE_UNAVAILABLE,
                )
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
            return Response(
                {"error": "redis_unavailable"},
                status=status.HTTP_503_SERVICE_UNAVAILABLE,
            )
        return Response({"ticket": ticket, "expires_in_secs": 60})
