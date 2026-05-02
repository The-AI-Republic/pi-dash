# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-runner session lifecycle + long-poll endpoints.

See ``.ai_design/move_to_https/design.md`` §7.1 / §7.3. Three verbs:

- ``POST /api/v1/runner/runners/<rid>/sessions/`` — open a session for
  this runner. Replaces the legacy WS Hello + attach steps.
- ``DELETE /api/v1/runner/runners/<rid>/sessions/<sid>/`` — clean
  shutdown.
- ``POST /api/v1/runner/runners/<rid>/sessions/<sid>/poll`` — long-poll
  for control-plane messages; body carries ``ack`` and ``status``.
"""

from __future__ import annotations

import logging
import time
import uuid as _uuid
from typing import Any, Dict, List

from django.conf import settings
from django.db import transaction
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.runner.authentication import RunnerAccessTokenAuthentication
from pi_dash.runner.models import RunnerSession, RunnerStatus
from pi_dash.runner.services import outbox, session_service

logger = logging.getLogger(__name__)
_POLL_SLICE_MS = 1000


class _SessionEvictedDuringPoll(Exception):
    pass


def _check_protocol_header(request) -> Response | None:
    raw = (request.headers.get("X-Runner-Protocol-Version") or "").strip()
    if not raw:
        return None
    try:
        version = int(raw)
    except ValueError:
        return Response(
            {
                "error": "protocol_version_unsupported",
                "minimum": settings.RUNNER_PROTOCOL_VERSION,
            },
            status=426,
        )
    if version < settings.RUNNER_PROTOCOL_VERSION:
        return Response(
            {
                "error": "protocol_version_unsupported",
                "minimum": settings.RUNNER_PROTOCOL_VERSION,
            },
            status=426,
        )
    return None


class RunnerSessionOpenEndpoint(APIView):
    """``POST /runners/<rid>/sessions/`` — open a session for one runner.

    Combines today's session-open + per-runner Hello (``design.md`` §7.1).
    """

    authentication_classes = [RunnerAccessTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def post(self, request, runner_id):
        proto_err = _check_protocol_header(request)
        if proto_err is not None:
            return proto_err
        runner = getattr(request, "auth_runner", None)
        if runner is None or str(runner.id) != str(runner_id):
            return Response(
                {"error": "runner_id_mismatch"},
                status=status.HTTP_403_FORBIDDEN,
            )

        body = request.data or {}
        # Validate project_slug **before** any session-row creation
        # (design.md §7.1 ordering note) so a mismatch doesn't leave a
        # phantom row.
        project_slug = body.get("project_slug")
        if project_slug:
            expected = session_service.resolve_runner_project_slug(runner)
            if expected and str(project_slug) != expected:
                return Response(
                    {"error": "project_mismatch", "expected": expected},
                    status=status.HTTP_409_CONFLICT,
                )

        # Keep Redis I/O OUT of the atomic() block below: ``select_for_update``
        # holds a row lock on the prior session for the txn's lifetime, and a
        # slow XCLAIM inside would leave Postgres ``idle in transaction`` until
        # Redis returns — observed jamming a row for an hour, blocking every
        # subsequent poll / open / delete on it.
        outbox.ensure_stream_group(runner.id)

        old_session_id = None
        new_sid = _uuid.uuid4()
        with transaction.atomic():
            prior = (
                RunnerSession.objects.select_for_update()
                .filter(runner=runner, revoked_at__isnull=True)
                .first()
            )
            if prior is not None:
                old_session_id = str(prior.id)
                prior.revoked_at = timezone.now()
                prior.revoked_reason = "evicted_by_new_session"
                prior.save(update_fields=["revoked_at", "revoked_reason"])

            RunnerSession.objects.create(
                id=new_sid,
                runner=runner,
                protocol_version=settings.RUNNER_PROTOCOL_VERSION,
                last_seen_at=timezone.now(),
            )

            session_service.apply_hello(runner, body)
            session_service.mark_runner_online(runner.id)

        # Post-tx Redis side effects — the new session row is committed and
        # visible, and the row lock is released, so a Redis hang here can no
        # longer wedge the database.
        if old_session_id:
            outbox.clear_session_marker(_uuid.UUID(old_session_id))
        outbox.claim_pending_for_new_session(
            runner_id=runner.id,
            old_consumer=outbox.consumer_name(old_session_id) if old_session_id else None,
            new_consumer=outbox.consumer_name(new_sid),
        )
        outbox.publish_session_eviction(
            runner.id,
            old_session_id=old_session_id,
            new_session_id=str(new_sid),
        )

        # 8. Drain queued runs into the live stream.
        try:
            from pi_dash.runner.services.matcher import drain_for_runner_by_id

            drain_for_runner_by_id(runner.id)
        except Exception:
            logger.exception("drain_for_runner_by_id failed for %s", runner.id)

        # 9. Drain offline buffer into live stream.
        outbox.drain_offline_into_live(runner.id)

        # 10. Resume in-flight run, if any.
        resume_ack = None
        in_flight = body.get("in_flight_run")
        if in_flight:
            resume_ack = session_service.build_resume_ack(runner, str(in_flight))

        welcome = {
            "type": "welcome",
            "rid": str(runner.id),
            "server_time": timezone.now().isoformat(),
            "long_poll_interval_secs": settings.LONG_POLL_INTERVAL_SECS,
            "protocol_version": settings.RUNNER_PROTOCOL_VERSION,
        }
        return Response(
            {
                "session_id": str(new_sid),
                "welcome": welcome,
                "resume_ack": resume_ack,
            },
            status=status.HTTP_201_CREATED,
        )


class RunnerSessionDeleteEndpoint(APIView):
    """``DELETE /runners/<rid>/sessions/<sid>/`` — clean shutdown."""

    authentication_classes = [RunnerAccessTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def delete(self, request, runner_id, sid):
        runner = getattr(request, "auth_runner", None)
        if runner is None or str(runner.id) != str(runner_id):
            return Response(
                {"error": "runner_id_mismatch"},
                status=status.HTTP_403_FORBIDDEN,
            )
        try:
            session = RunnerSession.objects.get(
                id=sid, runner=runner, revoked_at__isnull=True
            )
        except RunnerSession.DoesNotExist:
            outbox.clear_session_marker(sid)
            return Response(status=status.HTTP_204_NO_CONTENT)
        session.revoked_at = timezone.now()
        session.revoked_reason = "clean_shutdown"
        session.save(update_fields=["revoked_at", "revoked_reason"])
        session_service.mark_runner_offline(runner.id)
        outbox.clear_session_marker(sid)
        return Response(status=status.HTTP_204_NO_CONTENT)


class RunnerSessionPollEndpoint(APIView):
    """``POST /runners/<rid>/sessions/<sid>/poll`` — long-poll."""

    authentication_classes = [RunnerAccessTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def post(self, request, runner_id, sid):
        runner = getattr(request, "auth_runner", None)
        if runner is None or str(runner.id) != str(runner_id):
            return Response(
                {"error": "runner_id_mismatch"},
                status=status.HTTP_403_FORBIDDEN,
            )

        try:
            session = RunnerSession.objects.get(id=sid, runner=runner)
        except RunnerSession.DoesNotExist:
            return Response(
                {"error": "session_evicted"},
                status=status.HTTP_409_CONFLICT,
            )
        if session.revoked_at is not None:
            return Response(
                {"error": "session_evicted", "reason": session.revoked_reason},
                status=status.HTTP_409_CONFLICT,
            )

        body = request.data or {}
        ack_ids: List[str] = list(body.get("ack") or [])
        status_entry: Dict[str, Any] = body.get("status") or {}

        # 2. Update session.last_seen_at.
        session.last_seen_at = timezone.now()
        session.save(update_fields=["last_seen_at"])

        # 3. Update runner.last_heartbeat_at + reap stale busy runs.
        from pi_dash.runner.models import Runner

        Runner.objects.filter(pk=runner.id).update(
            last_heartbeat_at=timezone.now(),
            status=(
                RunnerStatus.BUSY
                if status_entry.get("status") == "busy"
                else RunnerStatus.ONLINE
            ),
        )
        if status_entry:
            session_service.reap_stale_busy_runs(runner, status_entry)
            # Volatile observability snapshot — see
            # `.ai_design/runner_agent_bridge/design.md` §4.5.2.
            # Pre-observability runners send no snapshot fields and the
            # helper short-circuits.
            session_service.upsert_runner_live_state(runner, status_entry)

        # 5. XACK explicit ids.
        if ack_ids:
            outbox.ack_for_session(runner.id, ack_ids)

        # 6. XREADGROUP — first poll uses 0 (replay PEL), subsequent
        # polls use >.
        use_zero = not outbox.is_pel_drained(sid)
        block_ms = max(
            1, settings.LONG_POLL_INTERVAL_SECS * 1000
        ) if not use_zero else 0
        try:
            messages = self._read_with_eviction_awareness(
                runner_id=runner.id,
                session_id=sid,
                sid=sid,
                block_ms=block_ms,
                use_zero=use_zero,
            )
        except _SessionEvictedDuringPoll:
            return Response(
                {"error": "session_evicted"},
                status=status.HTTP_409_CONFLICT,
            )
        if use_zero:
            outbox.mark_pel_drained(sid)

        return Response(
            {
                "messages": messages,
                "server_time": timezone.now().isoformat(),
                "long_poll_interval_secs": settings.LONG_POLL_INTERVAL_SECS,
            }
        )

    def _read_with_eviction_awareness(
        self,
        *,
        runner_id,
        session_id,
        sid,
        block_ms: int,
        use_zero: bool,
    ) -> list[dict]:
        if block_ms <= 0:
            return outbox.read_for_session(
                runner_id=runner_id,
                session_id=session_id,
                block_ms=0,
                count=100,
                use_zero=use_zero,
            )

        from pi_dash.settings.redis import redis_instance

        client = redis_instance()
        if client is None:
            return outbox.read_for_session(
                runner_id=runner_id,
                session_id=session_id,
                block_ms=block_ms,
                count=100,
                use_zero=use_zero,
            )

        pubsub = client.pubsub(ignore_subscribe_messages=True)
        pubsub.subscribe(outbox.session_eviction_channel(runner_id))
        deadline = time.monotonic() + (block_ms / 1000.0)
        try:
            while True:
                remaining_ms = max(0, int((deadline - time.monotonic()) * 1000))
                slice_ms = min(_POLL_SLICE_MS, remaining_ms)
                messages = outbox.read_for_session(
                    runner_id=runner_id,
                    session_id=session_id,
                    block_ms=slice_ms,
                    count=100,
                    use_zero=use_zero,
                )
                if messages or remaining_ms <= 0:
                    return messages
                use_zero = False
                if pubsub.get_message(timeout=0) is not None:
                    raise _SessionEvictedDuringPoll
        finally:
            pubsub.close()
