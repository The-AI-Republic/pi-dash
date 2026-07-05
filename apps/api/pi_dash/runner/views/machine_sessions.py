# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Machine-level session lifecycle + long-poll endpoints.

The machine-scoped twin of :mod:`pi_dash.runner.views.sessions`. The
daemon opens exactly one of these on startup — authenticated by the
shared ``mt_`` MachineToken, no runner required — and long-polls it for
machine-scoped control messages (``create_runner``, ``config_push``, …).
Because it is keyed on the ``DevMachine`` and not on any runner, the
channel exists even when the machine hosts zero runners, which is the
"add your first runner" case cloud-driven runner creation needs.

Three verbs, mirroring the per-runner session:

- ``POST   /api/v1/runner/dev-machines/<mid>/sessions/`` — open.
- ``DELETE /api/v1/runner/dev-machines/<mid>/sessions/<sid>/`` — clean
  shutdown.
- ``POST   /api/v1/runner/dev-machines/<mid>/sessions/<sid>/poll`` —
  long-poll; body carries ``ack``.
"""

from __future__ import annotations

import json
import logging
import time
import uuid as _uuid
from typing import Any, Dict, List

from asgiref.sync import sync_to_async
from django.conf import settings
from django.db import transaction
from django.http import JsonResponse
from django.utils import timezone
from rest_framework import exceptions as drf_exceptions
from rest_framework import status
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.runner.authentication import MachineTokenAuthentication
from pi_dash.runner.models import DevMachine, MachineSession
from pi_dash.runner.services import machine_outbox

logger = logging.getLogger(__name__)
_POLL_SLICE_MS = 1000


class _SessionEvictedDuringPoll(Exception):
    pass


def _auth_dev_machine(request, dev_machine_id) -> DevMachine | None:
    """Return the authenticated dev machine iff the presented MachineToken
    is bound to it, else ``None``.

    The shared ``mt_`` token carries the machine identity (``dev_machine``);
    a token minted for machine A must not drive machine B's session.
    """
    token = getattr(request, "auth_machine_token", None)
    if token is None or token.dev_machine_id is None:
        return None
    if str(token.dev_machine_id) != str(dev_machine_id):
        return None
    return token.dev_machine


class MachineSessionOpenEndpoint(APIView):
    """``POST /dev-machines/<mid>/sessions/`` — open a machine session."""

    authentication_classes = [MachineTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def post(self, request, dev_machine_id):
        machine = _auth_dev_machine(request, dev_machine_id)
        if machine is None:
            return Response(
                {"error": "dev_machine_mismatch"},
                status=status.HTTP_403_FORBIDDEN,
            )

        machine_outbox.ensure_stream_group(machine.id)

        old_session_id = None
        new_sid = _uuid.uuid4()
        with transaction.atomic():
            prior = (
                MachineSession.objects.select_for_update()
                .filter(dev_machine=machine, revoked_at__isnull=True)
                .first()
            )
            if prior is not None:
                old_session_id = str(prior.id)
                prior.revoked_at = timezone.now()
                prior.revoked_reason = "evicted_by_new_session"
                prior.save(update_fields=["revoked_at", "revoked_reason"])

            MachineSession.objects.create(
                id=new_sid,
                dev_machine=machine,
                protocol_version=settings.RUNNER_PROTOCOL_VERSION,
                last_seen_at=timezone.now(),
            )
            DevMachine.objects.filter(pk=machine.id).update(
                last_seen_at=timezone.now()
            )

        # Post-tx Redis side effects — the session row is committed and the
        # row lock released, so a slow Redis call can no longer wedge the DB.
        if old_session_id:
            machine_outbox.clear_session_marker(_uuid.UUID(old_session_id))
        machine_outbox.publish_session_eviction(
            machine.id,
            old_session_id=old_session_id,
            new_session_id=str(new_sid),
        )
        machine_outbox.drain_offline_into_live(machine.id)

        welcome = {
            "type": "welcome",
            "dev_machine_id": str(machine.id),
            "server_time": timezone.now().isoformat(),
            "long_poll_interval_secs": settings.LONG_POLL_INTERVAL_SECS,
            "protocol_version": settings.RUNNER_PROTOCOL_VERSION,
        }
        return Response(
            {"session_id": str(new_sid), "welcome": welcome},
            status=status.HTTP_201_CREATED,
        )


class MachineSessionDeleteEndpoint(APIView):
    """``DELETE /dev-machines/<mid>/sessions/<sid>/`` — clean shutdown."""

    authentication_classes = [MachineTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def delete(self, request, dev_machine_id, sid):
        machine = _auth_dev_machine(request, dev_machine_id)
        if machine is None:
            return Response(
                {"error": "dev_machine_mismatch"},
                status=status.HTTP_403_FORBIDDEN,
            )
        try:
            session = MachineSession.objects.get(
                id=sid, dev_machine=machine, revoked_at__isnull=True
            )
        except MachineSession.DoesNotExist:
            machine_outbox.clear_session_marker(sid)
            return Response(status=status.HTTP_204_NO_CONTENT)
        session.revoked_at = timezone.now()
        session.revoked_reason = "clean_shutdown"
        session.save(update_fields=["revoked_at", "revoked_reason"])
        machine_outbox.clear_session_marker(sid)
        return Response(status=status.HTTP_204_NO_CONTENT)


def _authenticate_poll_machine(request):
    """Run :class:`MachineTokenAuthentication` outside DRF.

    The long-poll view is a plain Django async view (DRF's ``APIView``
    cannot await), so the auth class is invoked directly. Raises
    ``AuthenticationFailed`` exactly like the DRF dispatch path.
    """
    result = MachineTokenAuthentication().authenticate(request)
    if result is None:
        return None
    return getattr(request, "auth_machine_token", None)


def _poll_bookkeeping(machine, body: Dict[str, Any], sid):
    """Sync pre-wait phase: session checks, machine presence, acks.

    Returns ``(error, plan)`` — exactly one is non-None.
    """
    try:
        session = MachineSession.objects.get(id=sid, dev_machine=machine)
    except MachineSession.DoesNotExist:
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

    now_ts = timezone.now()
    session.last_seen_at = now_ts
    session.save(update_fields=["last_seen_at"])
    DevMachine.objects.filter(pk=machine.id).update(last_seen_at=now_ts)

    if ack_ids:
        machine_outbox.ack_for_session(machine.id, ack_ids)

    use_zero = not machine_outbox.is_pel_drained(sid)
    block_ms = max(1, settings.LONG_POLL_INTERVAL_SECS * 1000) if not use_zero else 0
    return None, {"block_ms": block_ms, "use_zero": use_zero}


async def _aread_with_eviction_awareness(
    *,
    dev_machine_id,
    session_id,
    block_ms: int,
    use_zero: bool,
) -> list[dict]:
    """Await messages for the session, breaking early on eviction.

    Mirrors the per-runner poll's async read: runs on the event loop so
    the long block window holds no worker thread, and slices the block
    so ``BLOCK 0`` (block-forever) is never issued on an expired
    deadline.
    """
    if block_ms <= 0:
        if not use_zero:
            return []
        return await machine_outbox.aread_for_session(
            dev_machine_id=dev_machine_id,
            session_id=session_id,
            block_ms=0,
            count=100,
            use_zero=use_zero,
        )

    from pi_dash.settings.redis import async_redis_instance

    client = async_redis_instance()
    if client is None:
        return await machine_outbox.aread_for_session(
            dev_machine_id=dev_machine_id,
            session_id=session_id,
            block_ms=block_ms,
            count=100,
            use_zero=use_zero,
        )

    pubsub = client.pubsub(ignore_subscribe_messages=True)
    try:
        await pubsub.subscribe(
            machine_outbox.session_eviction_channel(dev_machine_id)
        )
        deadline = time.monotonic() + (block_ms / 1000.0)
        while True:
            remaining_ms = max(0, int((deadline - time.monotonic()) * 1000))
            if remaining_ms <= 0:
                return []
            slice_ms = min(_POLL_SLICE_MS, remaining_ms)
            messages = await machine_outbox.aread_for_session(
                dev_machine_id=dev_machine_id,
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


async def machine_session_poll(request, dev_machine_id, sid):
    """``POST /dev-machines/<mid>/sessions/<sid>/poll`` — long-poll.

    A plain Django **async** view (not DRF): the request parks up to
    ``LONG_POLL_INTERVAL_SECS`` waiting for control messages, and under
    ASGI a sync long poll would block every other request sharing the
    worker's ``thread_sensitive`` thread. Auth + DB bookkeeping stay
    sync (briefly, via ``sync_to_async``); only the wait is async.
    """
    if request.method != "POST":
        return JsonResponse(
            {"detail": f'Method "{request.method}" not allowed.'}, status=405
        )

    try:
        token = await sync_to_async(_authenticate_poll_machine)(request)
    except drf_exceptions.AuthenticationFailed as exc:
        return JsonResponse(
            {"detail": str(exc.detail)}, status=status.HTTP_401_UNAUTHORIZED
        )
    if (
        token is None
        or token.dev_machine_id is None
        or str(token.dev_machine_id) != str(dev_machine_id)
    ):
        return JsonResponse(
            {"error": "dev_machine_mismatch"}, status=status.HTTP_403_FORBIDDEN
        )
    machine = await sync_to_async(lambda: token.dev_machine)()

    body: Dict[str, Any] = {}
    if request.body:
        try:
            parsed = json.loads(request.body)
        except ValueError:
            return JsonResponse(
                {"detail": "JSON parse error"}, status=status.HTTP_400_BAD_REQUEST
            )
        if isinstance(parsed, dict):
            body = parsed

    error, plan = await sync_to_async(_poll_bookkeeping)(machine, body, sid)
    if error is not None:
        return JsonResponse(error["payload"], status=error["status"])

    try:
        messages = await _aread_with_eviction_awareness(
            dev_machine_id=machine.id,
            session_id=sid,
            block_ms=plan["block_ms"],
            use_zero=plan["use_zero"],
        )
    except _SessionEvictedDuringPoll:
        return JsonResponse(
            {"error": "session_evicted"}, status=status.HTTP_409_CONFLICT
        )
    if plan["use_zero"]:
        await sync_to_async(machine_outbox.mark_pel_drained)(sid)

    return JsonResponse(
        {
            "messages": messages,
            "server_time": timezone.now().isoformat(),
            "long_poll_interval_secs": settings.LONG_POLL_INTERVAL_SECS,
        }
    )


# See the per-runner poll view: ``@csrf_exempt`` wraps the coroutine in a
# sync function under Django 4.2 and hides it from async detection, so set
# the attribute directly. The daemon authenticates with a bearer token,
# never cookies, so CSRF does not apply.
machine_session_poll.csrf_exempt = True
