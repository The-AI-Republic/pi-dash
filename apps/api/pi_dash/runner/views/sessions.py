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

import json
import logging
import time
import uuid as _uuid
from typing import Any, Dict, List

from asgiref.sync import sync_to_async
from django.conf import settings
from django.db import OperationalError, connection, transaction
from django.http import JsonResponse
from django.utils import timezone
from redis.exceptions import RedisError
from rest_framework import exceptions as drf_exceptions
from rest_framework import status
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.runner.authentication import RunnerAccessTokenAuthentication
from pi_dash.runner.models import RunnerSession, RunnerStatus
from pi_dash.runner.services import chat as chat_service
from pi_dash.runner.services import outbox, session_service

logger = logging.getLogger(__name__)
_POLL_SLICE_MS = 1000
_REDIS_SIDE_EFFECT_ERRORS = (RedisError, OSError)


class _SessionEvictedDuringPoll(Exception):
    pass


def _bound_txn_waits() -> None:
    """Cap lock + statement waits for the CURRENT transaction (Postgres only).

    A session-open / poll transaction that blocks on a runner-scoped row
    lock with no timeout parks the worker until the reverse proxy 504s — and
    the worker keeps waiting long after the client is gone, while holding
    row locks of its own (e.g. the just-evicted prior session). Every runner
    retry then queues behind it: a self-sustaining per-runner convoy,
    observed in production as session-open hanging exactly 60s → 504 on
    every attempt for hours while all other runners stayed healthy.

    ``SET LOCAL`` scopes the caps to the enclosing transaction. On timeout
    Postgres raises ``OperationalError``; callers translate it to a 503 so
    the runner backs off and retries instead of feeding the convoy.
    """
    if connection.vendor != "postgresql":
        return
    lock_ms = int(getattr(settings, "RUNNER_TXN_LOCK_TIMEOUT_MS", 5000))
    stmt_ms = int(getattr(settings, "RUNNER_TXN_STATEMENT_TIMEOUT_MS", 20000))
    with connection.cursor() as cursor:
        cursor.execute(f"SET LOCAL lock_timeout = '{lock_ms}ms'")
        cursor.execute(f"SET LOCAL statement_timeout = '{stmt_ms}ms'")


def _session_open_side_effect(log_runner_id, label: str, func, *args, **kwargs):
    started = time.monotonic()
    try:
        return func(*args, **kwargs)
    except _REDIS_SIDE_EFFECT_ERRORS:
        logger.exception(
            "runner session-open Redis side effect failed runner=%s step=%s",
            log_runner_id,
            label,
        )
        return None
    finally:
        elapsed_ms = int((time.monotonic() - started) * 1000)
        warn_ms = int(getattr(settings, "RUNNER_SESSION_OPEN_REDIS_WARN_MS", 500))
        if elapsed_ms >= warn_ms:
            logger.warning(
                "runner session-open Redis side effect slow runner=%s step=%s duration_ms=%s",
                log_runner_id,
                label,
                elapsed_ms,
            )


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
        _session_open_side_effect(
            runner.id,
            "ensure_stream_group",
            outbox.ensure_stream_group,
            runner.id,
        )

        old_session_id = None
        new_sid = _uuid.uuid4()
        try:
            with transaction.atomic():
                _bound_txn_waits()
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
                released_chats = chat_service.release_active_chats_for_runner(
                    runner,
                    "runner opened a new session before the prior chat turn completed",
                )
                if released_chats:
                    logger.info(
                        "released %s stale active chat session(s) for runner %s on session open",
                        released_chats,
                        runner.id,
                    )
        except OperationalError:
            # Lock or statement timeout: another transaction is sitting on
            # this runner's rows. Fail fast so the runner retries with
            # backoff — waiting here is what builds the worker convoy.
            logger.exception(
                "runner session-open timed out waiting on runner-scoped locks runner=%s",
                runner.id,
            )
            return Response(
                {"error": "runner_state_locked"},
                status=status.HTTP_503_SERVICE_UNAVAILABLE,
            )

        # Post-tx Redis side effects — the new session row is committed and
        # visible, and the row lock is released, so a Redis hang here can no
        # longer wedge the database.
        if old_session_id:
            _session_open_side_effect(
                runner.id,
                "clear_session_marker",
                outbox.clear_session_marker,
                _uuid.UUID(old_session_id),
            )
        _session_open_side_effect(
            runner.id,
            "claim_pending_for_new_session",
            outbox.claim_pending_for_new_session,
            runner_id=runner.id,
            old_consumer=outbox.consumer_name(old_session_id) if old_session_id else None,
            new_consumer=outbox.consumer_name(new_sid),
        )
        _session_open_side_effect(
            runner.id,
            "publish_session_eviction",
            outbox.publish_session_eviction,
            runner.id,
            old_session_id=old_session_id,
            new_session_id=str(new_sid),
        )

        # 8. Drain offline buffer into live stream.
        _session_open_side_effect(
            runner.id,
            "drain_offline_into_live",
            outbox.drain_offline_into_live,
            runner.id,
        )

        # 9. Resume in-flight run, if any.
        resume_ack = None
        in_flight = body.get("in_flight_run")
        if in_flight:
            resume_ack = session_service.build_resume_ack(runner, str(in_flight))

        # 9b. Redeliver an outstanding assigned/waiting run the runner did not
        # report as in-flight (design §6.3). After a daemon restart the local
        # worktree queue is lost, so a run the cloud still has ASSIGNED /
        # WAITING_FOR_WORKTREE is invisible to the daemon until we push it
        # back. The runner treats this exactly like a fresh Assign. Old
        # runners ignore the unknown response key, so this is additive.
        redeliver = session_service.build_session_open_redeliver(
            runner, str(in_flight) if in_flight else None
        )

        welcome = {
            "type": "welcome",
            "rid": str(runner.id),
            "server_time": timezone.now().isoformat(),
            "long_poll_interval_secs": settings.LONG_POLL_INTERVAL_SECS,
            "protocol_version": settings.RUNNER_PROTOCOL_VERSION,
        }
        if settings.LATEST_RUNNER_VERSION:
            welcome["latest_runner_version"] = settings.LATEST_RUNNER_VERSION
        if settings.MIN_RUNNER_VERSION:
            welcome["min_runner_version"] = settings.MIN_RUNNER_VERSION
        return Response(
            {
                "session_id": str(new_sid),
                "welcome": welcome,
                "resume_ack": resume_ack,
                "redeliver": redeliver,
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
            session = RunnerSession.objects.get(id=sid, runner=runner, revoked_at__isnull=True)
        except RunnerSession.DoesNotExist:
            outbox.clear_session_marker(sid)
            return Response(status=status.HTTP_204_NO_CONTENT)
        session.revoked_at = timezone.now()
        session.revoked_reason = "clean_shutdown"
        session.save(update_fields=["revoked_at", "revoked_reason"])
        session_service.mark_runner_offline(runner.id)
        outbox.clear_session_marker(sid)
        return Response(status=status.HTTP_204_NO_CONTENT)


def _authenticate_poll_runner(request):
    """Run :class:`RunnerAccessTokenAuthentication` outside DRF.

    The long-poll view is a plain Django async view (DRF's ``APIView``
    cannot await), so the auth class is invoked directly. Raises
    ``rest_framework.exceptions.AuthenticationFailed`` exactly like the
    DRF dispatch path; returns the authenticated runner or ``None``
    when no bearer credential was presented.
    """
    result = RunnerAccessTokenAuthentication().authenticate(request)
    if result is None:
        return None
    return getattr(request, "auth_runner", None)


def _poll_bookkeeping(runner, body: Dict[str, Any], sid):
    """Sync pre-wait phase of the long poll: session checks, heartbeat,
    stale-run reaping, acks, and drain scheduling.

    Returns ``(error, plan)`` — exactly one is non-None. ``error`` is
    ``{"payload": ..., "status": ...}`` for an early rejection; ``plan``
    carries ``block_ms`` / ``use_zero`` for the async wait phase.
    """
    try:
        session = RunnerSession.objects.get(id=sid, runner=runner)
    except RunnerSession.DoesNotExist:
        return (
            {"payload": {"error": "session_evicted"}, "status": status.HTTP_409_CONFLICT},
            None,
        )
    if session.revoked_at is not None:
        return (
            {
                "payload": {"error": "session_evicted", "reason": session.revoked_reason},
                "status": status.HTTP_409_CONFLICT,
            },
            None,
        )

    ack_ids: List[str] = list(body.get("ack") or [])
    raw_status = body.get("status") or {}
    status_entry: Dict[str, Any] = raw_status if isinstance(raw_status, dict) else {}

    # 2. Update session.last_seen_at.
    session.last_seen_at = timezone.now()
    session.save(update_fields=["last_seen_at"])

    # 3. Update runner.last_heartbeat_at + reap stale busy runs.
    from pi_dash.runner.models import Runner
    from pi_dash.runner.services.matcher import (
        HEARTBEAT_GRACE,
        drain_for_runner_by_id,
    )

    # Capture prior heartbeat/status from the DB so we can detect when this
    # poll makes the runner eligible for queued work again.
    now_ts = timezone.now()
    try:
        with transaction.atomic():
            _bound_txn_waits()
            runner_snapshot = (
                Runner.objects.select_for_update().filter(pk=runner.id).values("last_heartbeat_at", "status").get()
            )
            prior_hb = runner_snapshot["last_heartbeat_at"]
            prior_status = runner_snapshot["status"]
            was_stale = prior_hb is None or (now_ts - prior_hb) > HEARTBEAT_GRACE
            reported_status = status_entry.get("status")
            reports_busy = reported_status == "busy"
            reports_available = reported_status in {"idle", "online"}
            became_available = prior_status == RunnerStatus.BUSY and reports_available
            status_allows_drain = reports_available or (not status_entry and prior_status != RunnerStatus.BUSY)

            runner_updates: Dict[str, Any] = {
                "last_heartbeat_at": now_ts,
                "status": RunnerStatus.BUSY if reports_busy else RunnerStatus.ONLINE,
            }
            # Capacity hint (design §6.4): persist the free-desk count the
            # runner reports for its work-dir pool so the matcher can prefer
            # runners with spare worktrees. Additive + optional — old runners
            # omit the field and we leave the column untouched.
            free_worktrees = session_service.parse_free_worktrees(status_entry.get("free_worktrees"))
            if free_worktrees is not None:
                runner_updates["free_worktrees"] = free_worktrees
            Runner.objects.filter(pk=runner.id).update(**runner_updates)
            if status_entry:
                session_service.reap_stale_busy_runs(runner, status_entry)
    except OperationalError:
        # Same convoy guard as session-open: a lock/statement timeout means
        # this runner's rows are contended — 503 so the daemon retries.
        logger.exception(
            "runner poll bookkeeping timed out waiting on runner-scoped locks runner=%s",
            runner.id,
        )
        return (
            {
                "payload": {"error": "runner_state_locked"},
                "status": status.HTTP_503_SERVICE_UNAVAILABLE,
            },
            None,
        )
    if status_entry:
        # Volatile observability snapshot — see
        # `.ai_design/runner_agent_bridge/design.md` §4.5.2.
        # Pre-observability runners send no snapshot fields and the
        # helper short-circuits. Failures here must never break the
        # poll path: a malformed snapshot or a transient DB error
        # would otherwise return 500 and spin the runner's retry loop.
        try:
            session_service.upsert_runner_live_state(runner, status_entry)
        except Exception:
            logger.exception(
                "upsert_runner_live_state failed for runner %s",
                runner.id,
            )

    # 5. XACK explicit ids.
    if ack_ids:
        outbox.ack_for_session(runner.id, ack_ids)

    # 6. XREADGROUP — first poll uses 0 (replay PEL), subsequent
    # polls use >.
    use_zero = not outbox.is_pel_drained(sid)

    # A session-open alone is not proof that the daemon is actually
    # polling. Drain on either stale-heartbeat recovery or the first
    # real poll for this session, after the heartbeat/status update
    # makes the runner assignable. Keep this as one trigger so a first
    # poll with no prior heartbeat does not double-dispatch.
    if (
        (was_stale or became_available or use_zero)
        and status_allows_drain
        and not status_entry.get("in_flight_run")
    ):

        def _drain_poll_ready_runner(rid=runner.id):
            try:
                drain_for_runner_by_id(rid)
            except Exception:
                logger.exception(
                    "drain_for_runner_by_id failed for poll-ready runner %s",
                    rid,
                )

        transaction.on_commit(_drain_poll_ready_runner)

    block_ms = max(1, settings.LONG_POLL_INTERVAL_SECS * 1000) if not use_zero else 0
    return None, {"block_ms": block_ms, "use_zero": use_zero}


async def _aread_with_eviction_awareness(
    *,
    runner_id,
    session_id,
    block_ms: int,
    use_zero: bool,
) -> list[dict]:
    """Await messages for the session, breaking early on eviction.

    Runs on the event loop (async Redis), so the long block window holds
    no worker thread — the whole point of the async poll view. A sync
    DRF view would pin the per-process ``thread_sensitive`` thread for
    the full block window, serializing every other sync request behind
    it (observed in production as 5s+ page loads with a handful of
    connected runners).
    """
    if block_ms <= 0:
        if not use_zero:
            return []
        # The initial PEL replay uses STREAMS ... 0 and must remain
        # nonblocking. The dangerous Redis case is BLOCK 0 with ">".
        return await outbox.aread_for_session(
            runner_id=runner_id,
            session_id=session_id,
            block_ms=0,
            count=100,
            use_zero=use_zero,
        )

    from pi_dash.settings.redis import async_redis_instance

    client = async_redis_instance()
    if client is None:
        return await outbox.aread_for_session(
            runner_id=runner_id,
            session_id=session_id,
            block_ms=block_ms,
            count=100,
            use_zero=use_zero,
        )

    pubsub = client.pubsub(ignore_subscribe_messages=True)
    try:
        await pubsub.subscribe(outbox.session_eviction_channel(runner_id))
        deadline = time.monotonic() + (block_ms / 1000.0)
        while True:
            remaining_ms = max(0, int((deadline - time.monotonic()) * 1000))
            # CRITICAL: bail BEFORE calling Redis when the deadline
            # has expired. `XREADGROUP BLOCK 0 STREAMS … >` means
            # "block forever" in Redis (BLOCK 0 is documented as
            # "block indefinitely"), not "do not block." If we let
            # `slice_ms` reach 0 and call aread_for_session, the
            # underlying XREADGROUP parks the request indefinitely —
            # well beyond the runner's HTTP timeout — and any assign
            # message that lands later gets claimed by this dead
            # consumer's PEL where the live session can never see it.
            # Observed in production: poll handlers stuck for 4+
            # hours on evicted sessions, blocking gunicorn workers
            # and reaping unrelated runs whose assigns couldn't
            # reach the live runner.
            if remaining_ms <= 0:
                return []
            slice_ms = min(_POLL_SLICE_MS, remaining_ms)
            messages = await outbox.aread_for_session(
                runner_id=runner_id,
                session_id=session_id,
                block_ms=slice_ms,
                count=100,
                use_zero=use_zero,
            )
            if messages:
                return messages
            use_zero = False
            if await pubsub.get_message(timeout=0) is not None:
                raise _SessionEvictedDuringPoll
    finally:
        await pubsub.close()


async def runner_session_poll(request, runner_id, sid):
    """``POST /runners/<rid>/sessions/<sid>/poll`` — long-poll.

    A plain Django **async** view rather than a DRF ``APIView``: the
    request spends up to ``LONG_POLL_INTERVAL_SECS`` (~25s) parked
    waiting for control-plane messages, and under ASGI every sync view
    in a worker shares one ``thread_sensitive`` thread — a sync long
    poll therefore blocks every other request on that worker for the
    whole window. Auth and DB bookkeeping stay sync (briefly, via
    ``sync_to_async``); only the wait is async.
    """
    if request.method != "POST":
        return JsonResponse({"detail": f'Method "{request.method}" not allowed.'}, status=405)

    try:
        runner = await sync_to_async(_authenticate_poll_runner)(request)
    except drf_exceptions.AuthenticationFailed as exc:
        return JsonResponse({"detail": str(exc.detail)}, status=status.HTTP_401_UNAUTHORIZED)
    if runner is None or str(runner.id) != str(runner_id):
        return JsonResponse({"error": "runner_id_mismatch"}, status=status.HTTP_403_FORBIDDEN)

    body: Dict[str, Any] = {}
    if request.body:
        try:
            parsed = json.loads(request.body)
        except ValueError:
            return JsonResponse({"detail": "JSON parse error"}, status=status.HTTP_400_BAD_REQUEST)
        if isinstance(parsed, dict):
            body = parsed

    error, plan = await sync_to_async(_poll_bookkeeping)(runner, body, sid)
    if error is not None:
        return JsonResponse(error["payload"], status=error["status"])

    try:
        messages = await _aread_with_eviction_awareness(
            runner_id=runner.id,
            session_id=sid,
            block_ms=plan["block_ms"],
            use_zero=plan["use_zero"],
        )
    except _SessionEvictedDuringPoll:
        return JsonResponse({"error": "session_evicted"}, status=status.HTTP_409_CONFLICT)
    if plan["use_zero"]:
        await sync_to_async(outbox.mark_pel_drained)(sid)

    return JsonResponse(
        {
            "messages": messages,
            "server_time": timezone.now().isoformat(),
            "long_poll_interval_secs": settings.LONG_POLL_INTERVAL_SECS,
        }
    )


# Django 4.2's ``@csrf_exempt`` wraps the view in a *sync* function, which
# hides the coroutine from the handler's async detection (the request then
# returns an un-awaited coroutine). Async decorator support only landed in
# Django 5.0, so mark the attribute directly. The runner daemon authenticates
# with a bearer token, never cookies, so CSRF does not apply — this mirrors
# DRF's csrf-exempt dispatch the endpoint had as an ``APIView``.
runner_session_poll.csrf_exempt = True
