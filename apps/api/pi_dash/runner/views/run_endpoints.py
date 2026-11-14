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
import uuid as _uuid
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
    Runner,
    RunnerSession,
)
from pi_dash.runner.services import capacity as capacity_service
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
        run = AgentRun.objects.select_related("work_item", "scheduler_binding").filter(id=run_id).first()
        if run is None:
            return None, Response({"error": "run_not_found"}, status=status.HTTP_404_NOT_FOUND)
        if not resolve_runner_for_run(run, request):
            return None, Response(
                {"error": "run_not_owned_by_runner"},
                status=status.HTTP_403_FORBIDDEN,
            )
        return run, None

    def _lock_non_terminal(self, run: AgentRun) -> tuple[Optional[AgentRun], Optional[Response]]:
        locked = AgentRun.objects.select_for_update().filter(pk=run.pk).first()
        if locked is None:
            return None, Response({"error": "run_not_found"}, status=status.HTTP_404_NOT_FOUND)
        if locked.status in run_lifecycle.TERMINAL_RUN_STATUSES:
            return locked, Response({"ok": True, "terminal": True})
        return locked, None

    @staticmethod
    def _cancellation_pending(locked: AgentRun) -> Optional[Response]:
        if locked.status == AgentRunStatus.CANCEL_REQUESTED:
            return Response({"ok": True, "cancel_requested": True, "ignored": True})
        return None

    def _fence_attempt(self, request, run: AgentRun, *, require_session: bool = False) -> Optional[Response]:
        """Protocol-1 attempt fencing (T2). Returns an error Response or None.

        Must be called inside ``transaction.atomic()`` — under the
        Runner → active RunnerSession → AgentRun lock order, with no
        I/O while locked. Authenticated ownership was already proven
        by :meth:`_resolve`; this revalidates under lock because the
        attempt may have been reassigned since. Protocol-0 keeps the
        existing compatibility behaviour (no checks at all).

        For protocol-1 attempts the assignment token must match
        exactly (``409 stale_assignment`` otherwise — tokenless
        requests cannot mutate). When ``require_session`` (admission /
        Accept / Started), ``session_id`` is additionally mandatory
        (``400`` when missing/invalid, ``409 session_evicted`` when it
        is not the runner's current active session). Terminal
        endpoints pass ``require_session=False``: outbox retries may
        use the current reopened session with the original attempt
        token.
        """
        runner = getattr(request, "auth_runner", None)
        if runner is None:
            return None
        locked_runner = Runner.objects.select_for_update().filter(pk=runner.pk).first()
        if locked_runner is None:
            return Response(
                {"error": "session_evicted"},
                status=status.HTTP_409_CONFLICT,
            )
        locked = AgentRun.objects.select_for_update().filter(pk=run.pk).first()
        if locked is None:
            return Response({"error": "run_not_found"}, status=status.HTTP_404_NOT_FOUND)
        # Protocol is immutable per attempt: a legacy attempt skips
        # every check below and keeps its existing behaviour exactly.
        if locked.assignment_protocol != capacity_service.PROTOCOL_MACHINE_QUEUE:
            return None
        data = request.data or {}
        if require_session:
            raw_session = str(data.get("session_id") or "").strip()
            if not raw_session:
                return Response(
                    {"error": "session_id_required"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
            session_uuid = capacity_service.parse_uuid(raw_session)
            if session_uuid is None:
                return Response(
                    {"error": "invalid_session_id"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
            active_sid = (
                RunnerSession.objects.filter(runner=locked_runner, revoked_at__isnull=True)
                .values_list("id", flat=True)
                .first()
            )
            if active_sid is None or str(active_sid) != str(session_uuid):
                return Response(
                    {"error": "session_evicted"},
                    status=status.HTTP_409_CONFLICT,
                )
        provided = (
            capacity_service.parse_uuid(data.get("assignment_token"))
            if data.get("assignment_token") is not None
            else None
        )
        if data.get("assignment_token") is not None and provided is None:
            return Response(
                {"error": "invalid_assignment_token"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not capacity_service.token_matches(locked, provided):
            return Response(
                {"error": "stale_assignment"},
                status=status.HTTP_409_CONFLICT,
            )
        return None


class RunAcceptEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if fence_err := self._fence_attempt(request, run, require_session=True):
                return fence_err
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            if pending := self._cancellation_pending(locked):
                return pending
            # Accept clears the waiting presentation (T2): the run is
            # now executing, not waiting for machine capacity.
            AgentRun.objects.filter(pk=locked.pk).update(
                status=AgentRunStatus.RUNNING,
                capacity_wait=None,
            )
        return Response({"ok": True})


class RunQueuedEndpoint(_RunEndpointBase):
    """``POST /runs/<run_id>/queued`` — retired worktree-queue notice.

    The worktree pool is retired (PDASHOSS01-137): a runner no longer queues
    locally for a lease, so there is nothing to record and no new run may
    enter ``WAITING_FOR_WORKTREE``. Pre-retirement daemons may still POST here
    after this ships, so the endpoint stays and acknowledges the post (never
    404s it and never makes the daemon retry), but it performs no state
    transition and records nothing from the post body. A run simply stays
    ``ASSIGNED`` until the runner posts ``accept`` (which drives ``RUNNING``).
    Historical rows that already hold ``WAITING_FOR_WORKTREE`` are untouched
    and keep rendering / redelivering via their status-set membership.
    """

    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        # Acknowledge and drop. There is no side effect to guard, so the
        # dedupe/lock machinery the transition used is no longer needed.
        return Response({"ok": True, "ignored": True})


class RunStartedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if fence_err := self._fence_attempt(request, run, require_session=True):
                return fence_err
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            if pending := self._cancellation_pending(locked):
                return pending
            thread_id = (request.data.get("thread_id") or "")[:128]
            # ``local_thread_id`` defaults to ``thread_id`` for older runners
            # that report only the resume handle; ``local_session_id`` and
            # ``agent_kind`` are new keys the run bridge reports at start.
            # agent_metadata is write-once here and never cleared on retry (so
            # a failed attempt's session id survives), in contrast to the
            # ``thread_id`` write, which run_lifecycle nulls on the parent when
            # a resume is unavailable.
            local_thread_id = (request.data.get("local_thread_id") or thread_id or "")[:128]
            local_session_id = (request.data.get("local_session_id") or "")[:128]
            agent_kind = (request.data.get("agent_kind") or "")[:24]
            updates = {
                "status": AgentRunStatus.RUNNING,
                "thread_id": thread_id,
                "started_at": timezone.now(),
                "agent_metadata": {
                    "local_session_id": local_session_id,
                    "local_thread_id": local_thread_id,
                    "agent_kind": agent_kind,
                },
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
            if fence_err := self._fence_attempt(request, run):
                return fence_err
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            if pending := self._cancellation_pending(locked):
                return pending
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
            AgentRun.objects.filter(pk=locked.pk).update(status=AgentRunStatus.AWAITING_APPROVAL)
        return Response({"ok": True})


class RunAwaitingReauthEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if fence_err := self._fence_attempt(request, run):
                return fence_err
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            if pending := self._cancellation_pending(locked):
                return pending
            AgentRun.objects.filter(pk=locked.pk).update(status=AgentRunStatus.AWAITING_REAUTH)
        return Response({"ok": True})


class RunCompletedEndpoint(_RunEndpointBase):
    def post(self, request, run_id):
        run, err = self._resolve(request, run_id)
        if err:
            return err
        with transaction.atomic():
            if fence_err := self._fence_attempt(request, run):
                return fence_err
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
            if fence_err := self._fence_attempt(request, run):
                return fence_err
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            if pending := self._cancellation_pending(locked):
                return pending
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
            if fence_err := self._fence_attempt(request, run):
                return fence_err
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
                if locked.status == AgentRunStatus.CANCEL_REQUESTED:
                    # The daemon has proved it has no resumable process for
                    # this run, which satisfies the cancellation barrier.
                    run_lifecycle.finalize_run_terminal(
                        runner,
                        locked.id,
                        AgentRunStatus.CANCELLED,
                    )
                    return Response({"ok": True, "cancelled": True})
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
                if locked.status == AgentRunStatus.CANCEL_REQUESTED:
                    # This assignment never started on the daemon, so there
                    # is no source process left to wait for.
                    run_lifecycle.finalize_run_terminal(
                        runner,
                        locked.id,
                        AgentRunStatus.CANCELLED,
                    )
                    return Response({"ok": True, "cancelled": True})
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
            if fence_err := self._fence_attempt(request, run):
                return fence_err
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
            if fence_err := self._fence_attempt(request, run):
                return fence_err
            if not _record_dedupe(run, _idempotency_key(request)):
                return Response({"ok": True, "duplicate": True})
            locked, closed = self._lock_non_terminal(run)
            if closed:
                return closed
            if pending := self._cancellation_pending(locked):
                return pending
            AgentRun.objects.filter(pk=locked.pk).update(status=AgentRunStatus.RUNNING)
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
                        "expires_at": (timezone.now() + timedelta(seconds=60)).isoformat(),
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


class RunCapacityEndpoint(_RunEndpointBase):
    """``POST /runs/<run_id>/capacity/`` — dev-machine capacity contract.

    ``.ai_design/dev_machine_run_queue/implementation.md`` §3 governs
    the wire format. Actions ``waiting`` / ``admit`` / ``defer`` /
    ``reconcile``; every response carries ``decision``, ``status``,
    ``assignment_token``, ``assignment_protocol`` and ``revision``
    (null where unavailable), and ``proceed`` additionally carries
    ``admission_id`` (the admit call's idempotency UUID).

    Lock order is Runner → active RunnerSession → AgentRun with no
    I/O while locked; the session fence rejects evicted sessions with
    ``409 session_evicted``. Protocol-1 requires the exact token;
    ``reconcile`` may omit a token only for legacy adoption (never
    ``running``). Duplicate admits are revalidated under lock — a
    repeated current admit returns the same ``admission_id`` and a
    cached/obsolete ``proceed`` is never returned.
    """

    def post(self, request, run_id):
        # 16 KiB body limit (§3). The content-length check trips before
        # DRF parses; the re-encoded check below covers clients that
        # omit the header.
        content_length = request.META.get("CONTENT_LENGTH")
        try:
            body_len = int(content_length) if content_length is not None else None
        except (TypeError, ValueError):
            body_len = None
        if body_len is not None and body_len > capacity_service.CAPACITY_BODY_LIMIT_BYTES:
            return Response(
                {"error": "body_too_large"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        idempotency_key = _idempotency_key(request)
        if not idempotency_key:
            return Response(
                {"error": "idempotency_key_required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if len(request.body) > capacity_service.CAPACITY_BODY_LIMIT_BYTES:
            return Response(
                {"error": "body_too_large"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        run, err = self._resolve(request, run_id)
        if err:
            return err
        data = request.data or {}
        action = str(data.get("action") or "").strip().lower()
        if action not in capacity_service.CAPACITY_ACTIONS:
            return Response(
                {"error": "invalid_action"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        raw_session = str(data.get("session_id") or "").strip()
        if not raw_session:
            return Response(
                {"error": "session_id_required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        session_uuid = capacity_service.parse_uuid(raw_session)
        if session_uuid is None:
            return Response(
                {"error": "invalid_session_id"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        revision = capacity_service.parse_revision(data.get("revision"))
        if revision is None:
            return Response(
                {"error": "revision_invalid"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        token_raw = data.get("assignment_token")
        provided_token = None
        if token_raw is not None:
            provided_token = capacity_service.parse_uuid(token_raw)
            if provided_token is None:
                return Response(
                    {"error": "invalid_assignment_token"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
        # Admit's admission_id is the idempotency UUID (§3), so the
        # key must be a UUID for that action.
        if action == "admit" and capacity_service.parse_uuid(idempotency_key) is None:
            return Response(
                {"error": "invalid_idempotency_key"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        # Informational client queue fields: validated when present,
        # defaulted when absent.
        queued_at = data.get("queued_at")
        if queued_at is not None:
            from datetime import datetime

            try:
                datetime.fromisoformat(str(queued_at).replace("Z", "+00:00"))
            except (ValueError, AttributeError):
                return Response(
                    {"error": "invalid_queued_at"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
            queued_at = str(queued_at)
        queue_position = None
        if data.get("queue_position") is not None:
            queue_position = capacity_service.validate_count(data.get("queue_position"), allow_zero=False)
            if queue_position is None:
                return Response(
                    {"error": "invalid_queue_position"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
        active = None
        if data.get("active") is not None:
            active = capacity_service.validate_count(data.get("active"), allow_zero=True)
            if active is None:
                return Response(
                    {"error": "invalid_active"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
        limit = None
        if data.get("limit") is not None:
            limit = capacity_service.validate_count(data.get("limit"), allow_zero=False)
            if limit is None or not (
                capacity_service.CAPACITY_MIN_LIMIT <= limit <= capacity_service.CAPACITY_MAX_LIMIT
            ):
                return Response(
                    {"error": "invalid_limit"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
        try:
            with transaction.atomic():
                capacity_service.bound_txn_waits()
                # Retry recognition: the dedupe row does not short-circuit
                # the response — the decision below is always recomputed
                # for the current row state ("never return cached/
                # obsolete proceed"). The flag distinguishes a retry of
                # an earlier call from a fresh equal-revision report.
                is_retry = not _record_dedupe(run, idempotency_key)
                decision, admission_id = self._decide(
                    request,
                    run,
                    action=action,
                    session_uuid=session_uuid,
                    provided_token=provided_token,
                    revision=revision,
                    idempotency_key=idempotency_key,
                    is_retry=is_retry,
                    queued_at=queued_at,
                    queue_position=queue_position,
                    active=active,
                    limit=limit,
                    data=data,
                )
                if isinstance(decision, Response):
                    return decision
        except _CapacitySessionEvicted:
            # Evicted (or never-current) session: the daemon must
            # reopen its session before retrying, even with valid
            # runner credentials.
            return Response(
                {"error": "session_evicted"},
                status=status.HTTP_409_CONFLICT,
            )
        except _CapacityRunGone:
            return Response({"error": "run_not_found"}, status=status.HTTP_404_NOT_FOUND)
        except _CapacityNotOwned:
            return Response(
                {"error": "run_not_owned_by_runner"},
                status=status.HTTP_403_FORBIDDEN,
            )
        except Exception as exc:  # lock/statement timeout → 503 convention
            from django.db import OperationalError

            if isinstance(exc, OperationalError):
                logger.exception("capacity endpoint timed out on runner locks run=%s", run_id)
                return Response(
                    {"error": "runner_state_locked"},
                    status=status.HTTP_503_SERVICE_UNAVAILABLE,
                )
            raise
        else:
            locked = AgentRun.objects.filter(pk=run.pk).first()
            if locked is None:
                return Response({"error": "run_not_found"}, status=status.HTTP_404_NOT_FOUND)
            return self._decision_response(decision, locked, admission_id=admission_id)

    def _decision_response(self, decision, locked_run, *, admission_id=None):
        cw = locked_run.capacity_wait if isinstance(locked_run.capacity_wait, dict) else None
        body = {
            "decision": decision,
            "status": locked_run.status,
            "assignment_token": str(locked_run.assignment_token) if locked_run.assignment_token is not None else None,
            "assignment_protocol": int(locked_run.assignment_protocol or 0),
            "revision": cw.get("revision") if cw else None,
        }
        if decision == "proceed":
            body["admission_id"] = admission_id
        return Response(body, status=status.HTTP_200_OK)

    def _decide(
        self,
        request,
        run,
        *,
        action,
        session_uuid,
        provided_token,
        revision,
        idempotency_key,
        is_retry,
        queued_at,
        queue_position,
        active,
        limit,
        data,
    ):
        """Apply one capacity action under lock; returns (decision, admission_id)."""
        from django.utils import timezone as _tz

        runner = getattr(request, "auth_runner", None)
        # Lock order: Runner → active RunnerSession → AgentRun. No I/O
        # happens below this point (pure row reads/writes + clock).
        locked_runner = Runner.objects.select_for_update().filter(pk=runner.pk).first()
        if locked_runner is None:
            # unreachable in practice: auth proved the row existed
            raise _CapacitySessionEvicted
        session = (
            RunnerSession.objects.select_for_update().filter(runner=locked_runner, revoked_at__isnull=True).first()
        )
        if session is None or str(session.id) != str(session_uuid):
            raise _CapacitySessionEvicted
        locked = AgentRun.objects.select_for_update().filter(pk=run.pk).first()
        if locked is None:
            raise _CapacityRunGone
        if locked.runner_id != locked_runner.pk:
            # Ownership changed between _resolve and the lock.
            raise _CapacityNotOwned
        now = _tz.now()
        cw = locked.capacity_wait if isinstance(locked.capacity_wait, dict) else None
        stored_revision = cw.get("revision") if cw else None
        stored_phase = cw.get("phase") if cw else None
        stored_session = cw.get("session_id") if cw else None
        stored_admission = cw.get("admission_id") if cw else None

        # Status dispositions first: terminal/canceled/stale/evicted
        # responses can never launch work.
        if locked.status in run_lifecycle.TERMINAL_RUN_STATUSES:
            if locked.capacity_wait is not None:
                locked.capacity_wait = None
                AgentRun.objects.filter(pk=locked.pk).update(capacity_wait=None)
            return "terminal", None
        if locked.status == AgentRunStatus.CANCEL_REQUESTED:
            return "cancel", None
        if locked.status in (
            AgentRunStatus.RUNNING,
            AgentRunStatus.AWAITING_APPROVAL,
            AgentRunStatus.AWAITING_REAUTH,
            AgentRunStatus.PAUSED_AWAITING_INPUT,
        ):
            return "already_started", None

        is_protocol_one = locked.assignment_protocol == capacity_service.PROTOCOL_MACHINE_QUEUE

        # Legacy attempts (protocol 0): only reconcile may touch them
        # (adoption); every other action is refused as in-flight until
        # adoption. Reconcile may omit the token solely for adoption.
        if not is_protocol_one:
            return self._decide_legacy(
                request,
                locked,
                action=action,
                provided_token=provided_token,
                revision=revision,
                idempotency_key=idempotency_key,
                is_retry=is_retry,
                queued_at=queued_at,
                queue_position=queue_position,
                active=active,
                limit=limit,
                data=data,
            )

        if not capacity_service.token_matches(locked, provided_token):
            return "stale_assignment", None

        if locked.status != AgentRunStatus.ASSIGNED and locked.status != AgentRunStatus.WAITING_FOR_WORKTREE:
            # Queued-into-a-runner states other than assigned (e.g. queued
            # rows are filtered by _resolve ownership, but be defensive).
            return "stale_assignment", None

        if stored_revision is not None and revision < stored_revision and not is_retry:
            return "stale_report", None

        if action == "waiting":
            return self._store_waiting(
                locked,
                cw,
                stored_phase=stored_phase,
                revision=revision,
                session_uuid=session_uuid,
                queued_at=queued_at,
                queue_position=queue_position,
                active=active,
                limit=limit,
                now=now,
            )
        if action == "admit":
            return self._admit(
                locked,
                cw,
                stored_phase=stored_phase,
                stored_session=stored_session,
                stored_admission=stored_admission,
                stored_revision=stored_revision,
                revision=revision,
                idempotency_key=idempotency_key,
                session_uuid=session_uuid,
                queued_at=queued_at,
                active=active,
                limit=limit,
                now=now,
            )
        if action == "defer":
            return self._defer(
                locked,
                cw,
                stored_phase=stored_phase,
                stored_revision=stored_revision,
                revision=revision,
                is_retry=is_retry,
                session_uuid=session_uuid,
                queued_at=queued_at,
                queue_position=queue_position,
                active=active,
                limit=limit,
                now=now,
                data=data,
            )
        # reconcile (protocol-1, token matched): confirm the
        # never-started assigned attempt; reset an abandoned preparing
        # report only after a session change.
        if stored_phase == capacity_service.PHASE_PREPARING and stored_session != str(session_uuid):
            self._save_wait(
                locked,
                session_uuid=session_uuid,
                revision=revision,
                queued_at=queued_at,
                queue_position=queue_position,
                active=active,
                limit=limit,
                now=now,
            )
            return "retain", None
        return "retain", None

    def _decide_legacy(
        self,
        request,
        locked,
        *,
        action,
        provided_token,
        revision,
        idempotency_key,
        is_retry,
        queued_at,
        queue_position,
        active,
        limit,
        data,
    ):
        """Capacity actions against an unadopted legacy attempt.

        Only ``reconcile`` without a token may proceed (adoption);
        anything else — or adoption conditions unmet — answers
        ``legacy_in_flight`` so the daemon follows the upgrade path
        instead of starting legacy work through the queue.
        """
        if provided_token is not None:
            return "stale_assignment", None
        if action != "reconcile":
            return "legacy_in_flight", None
        # Adoption: still assigned, never accepted, no admission in
        # flight (the caller's session is the active one — enforced by
        # the fence above), and never a running attempt. Anything else
        # stays in flight until it finishes.
        cw = locked.capacity_wait if isinstance(locked.capacity_wait, dict) else None
        phase = cw.get("phase") if cw else None
        if locked.status != AgentRunStatus.ASSIGNED or phase == capacity_service.PHASE_PREPARING:
            return "legacy_in_flight", None
        # Adopt: fresh token + protocol 1, fresh attempt (no stored
        # phase/revision — the daemon seeds its revision from the
        # response and reports waiting next).
        locked.assignment_token = _uuid.uuid4()
        locked.assignment_protocol = capacity_service.PROTOCOL_MACHINE_QUEUE
        locked.capacity_wait = None
        AgentRun.objects.filter(pk=locked.pk).update(
            assignment_token=locked.assignment_token,
            assignment_protocol=locked.assignment_protocol,
            capacity_wait=None,
        )
        return "retain", None

    def _save_wait(
        self,
        locked,
        *,
        session_uuid,
        revision,
        queued_at,
        queue_position,
        active,
        limit,
        now,
        phase=capacity_service.PHASE_WAITING,
        admission_id=None,
        prior=None,
    ):
        cw = {
            "phase": phase,
            "assignment_token": str(locked.assignment_token) if locked.assignment_token is not None else None,
            "session_id": str(session_uuid),
            "revision": revision,
            "queued_at": queued_at if queued_at is not None else (prior or {}).get("queued_at"),
            "reported_at": now.isoformat(),
            "queue_position": queue_position,
            "active": active,
            "limit": limit,
        }
        if admission_id is not None:
            cw["admission_id"] = admission_id
        locked.capacity_wait = cw
        AgentRun.objects.filter(pk=locked.pk).update(capacity_wait=cw)

    def _store_waiting(
        self, locked, cw, *, stored_phase, revision, session_uuid, queued_at, queue_position, active, limit, now
    ):
        # A waiting report never downgrades a current preparing phase;
        # it only refreshes freshness/counts. A fresh waiting store
        # otherwise records the queue position for presentation.
        if stored_phase == capacity_service.PHASE_PREPARING and cw:
            refreshed = dict(cw)
            refreshed.update(
                {
                    "revision": revision,
                    "reported_at": now.isoformat(),
                    "queue_position": queue_position if queue_position is not None else cw.get("queue_position"),
                    "active": active if active is not None else cw.get("active"),
                    "limit": limit if limit is not None else cw.get("limit"),
                }
            )
            locked.capacity_wait = refreshed
            AgentRun.objects.filter(pk=locked.pk).update(capacity_wait=refreshed)
            return "recorded", None
        self._save_wait(
            locked,
            session_uuid=session_uuid,
            revision=revision,
            queued_at=queued_at,
            queue_position=queue_position,
            active=active,
            limit=limit,
            now=now,
            prior=cw,
        )
        return "recorded", None

    def _admit(
        self,
        locked,
        cw,
        *,
        stored_phase,
        stored_session,
        stored_admission,
        stored_revision,
        revision,
        idempotency_key,
        session_uuid,
        queued_at,
        active,
        limit,
        now,
    ):
        if stored_revision is not None and revision < stored_revision:
            return "stale_report", None
        if stored_phase == capacity_service.PHASE_PREPARING:
            # Repeated current admit (same key) returns the same
            # admission_id after revalidation; a later request cannot
            # overwrite without defer or recovery reconciliation.
            if stored_session == str(session_uuid) and stored_admission == idempotency_key:
                return "proceed", stored_admission
            return "stale_report", None
        admission_id = idempotency_key
        self._save_wait(
            locked,
            session_uuid=session_uuid,
            revision=revision,
            queued_at=queued_at,
            queue_position=None,
            active=active,
            limit=limit,
            now=now,
            phase=capacity_service.PHASE_PREPARING,
            admission_id=admission_id,
            prior=cw,
        )
        return "proceed", admission_id

    def _defer(
        self,
        locked,
        cw,
        *,
        stored_phase,
        stored_revision,
        revision,
        is_retry,
        session_uuid,
        queued_at,
        queue_position,
        active,
        limit,
        now,
        data,
    ):
        if stored_phase != capacity_service.PHASE_PREPARING:
            # Nothing in flight: deferring is already-applied.
            return "recorded", None
        raw_admission = str(data.get("admission_id") or "").strip()
        if not raw_admission:
            return Response({"error": "admission_id_required"}, status=status.HTTP_400_BAD_REQUEST), None
        admission_uuid = capacity_service.parse_uuid(raw_admission)
        if admission_uuid is None:
            return Response({"error": "invalid_admission_id"}, status=status.HTTP_400_BAD_REQUEST), None
        stored_admission = cw.get("admission_id") if cw else None
        if str(admission_uuid) != str(stored_admission):
            return "stale_report", None
        if stored_revision is not None and revision <= stored_revision and not is_retry:
            return "stale_report", None
        self._save_wait(
            locked,
            session_uuid=session_uuid,
            revision=revision,
            queued_at=queued_at,
            queue_position=queue_position,
            active=active,
            limit=limit,
            now=now,
            prior=cw,
        )
        return "recorded", None


class _CapacitySessionEvicted(Exception):
    pass


class _CapacityRunGone(Exception):
    pass


class _CapacityNotOwned(Exception):
    pass
