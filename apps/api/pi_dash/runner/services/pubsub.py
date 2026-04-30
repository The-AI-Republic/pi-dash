# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Channels group-send helpers used by orchestrator code (Celery tasks, views)
to push messages onto a connected runner's WebSocket.

Usage:

    from pi_dash.runner.services.pubsub import send_to_runner
    send_to_runner(runner_id, {"type": "assign", ...})

The corresponding group is created in :class:`RunnerConsumer` as
``runner.<runner_id>``.
"""

from __future__ import annotations

import logging
from typing import Any, Dict
from uuid import UUID

from asgiref.sync import async_to_sync
from channels.layers import get_channel_layer

logger = logging.getLogger(__name__)


def runner_group(runner_id: UUID | str) -> str:
    return f"runner.{runner_id}"


def send_to_runner(runner_id: UUID | str, message: Dict[str, Any]) -> None:
    """Best-effort fire-and-forget send to a runner's current WS process.

    The ``runner_id`` is stamped onto the channels event so the
    consumer can tag the outbound envelope's ``rid`` correctly even
    in multi-runner mode where one consumer serves N runner groups.
    """
    layer = get_channel_layer()
    if layer is None:
        logger.warning("channel layer not configured; cannot route to %s", runner_id)
        return
    async_to_sync(layer.group_send)(runner_group(runner_id), {
        "type": "runner.send",
        "runner_id": str(runner_id),
        "payload": message,
    })


async def asend_to_runner(runner_id: UUID | str, message: Dict[str, Any]) -> None:
    layer = get_channel_layer()
    if layer is None:
        logger.warning("channel layer not configured; cannot route to %s", runner_id)
        return
    await layer.group_send(runner_group(runner_id), {
        "type": "runner.send",
        "runner_id": str(runner_id),
        "payload": message,
    })


def close_runner_session(runner_id: UUID | str, code: int = 4010) -> None:
    """Tell any connected consumer for this runner to close its WebSocket.

    Used after credential rotation / revocation to drop sessions still bound
    to an old secret.
    """
    layer = get_channel_layer()
    if layer is None:
        logger.warning("channel layer not configured; cannot close %s", runner_id)
        return
    async_to_sync(layer.group_send)(runner_group(runner_id), {
        "type": "runner.close",
        "code": code,
    })


def send_connection_revoke(
    runner_id: UUID | str, reason: str = "connection revoked"
) -> None:
    """Send a connection-scoped ``Revoke`` frame to the consumer that
    serves ``runner_id``, then close the WebSocket.

    Used by ``Connection.revoke()`` so the daemon receives a wire-level
    Revoke (rid=None) rather than just a TCP close, lets its supervisor
    initiate ``state.shutdown()`` cleanly, and exits non-zero. One
    consumer is joined to N runner groups on a connection; sending to
    any single owned runner reaches the same consumer once.
    """
    layer = get_channel_layer()
    if layer is None:
        logger.warning("channel layer not configured; cannot revoke %s", runner_id)
        return
    async_to_sync(layer.group_send)(runner_group(runner_id), {
        "type": "runner.revoke",
        "reason": reason,
    })
