# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Helpers for routing control messages to a runner.

Per ``.ai_design/move_to_https/design.md`` Phase 5, the always-on
WebSocket control plane is retired. Control traffic moves to Redis
Streams via :mod:`pi_dash.runner.services.outbox`. This module exposes
the small set of verbs the orchestrator uses (``send_to_runner``,
``close_runner_session``, ``send_runner_revoke``) as thin wrappers
over that outbox.
"""

from __future__ import annotations

import logging
import uuid as _uuid
from typing import Any, Dict
from uuid import UUID

from pi_dash.runner.services.machine_outbox import (
    MachineOfflineError,
    enqueue_for_machine,
)
from pi_dash.runner.services.outbox import (
    RunnerOfflineError,
    enqueue_for_runner,
)

logger = logging.getLogger(__name__)


def runner_group(runner_id: UUID | str) -> str:
    """Legacy Channels group name; retained for the upgrade-ticket WS."""
    return f"runner.{runner_id}"


def _ensure_envelope(message: Dict[str, Any]) -> Dict[str, Any]:
    body = dict(message)
    body.setdefault("mid", str(_uuid.uuid4()))
    return body


def send_to_runner(runner_id: UUID | str, message: Dict[str, Any]) -> None:
    """Best-effort enqueue of a control message for the runner.

    Routes through the outbox: live stream when the runner has an
    active session, offline buffer when it does not. Offline-rejected
    types (``assign``/``cancel``/``decide``/``resume_ack``) raise
    :class:`RunnerOfflineError`; callers in matcher/orchestrator are
    expected to re-queue the corresponding domain row.
    """
    try:
        enqueue_for_runner(runner_id, _ensure_envelope(message))
    except RunnerOfflineError:
        # Caller (matcher) handles requeue.
        raise
    except Exception:
        logger.exception("send_to_runner enqueue failed for %s", runner_id)


def send_to_machine(dev_machine_id: UUID | str, message: Dict[str, Any]) -> str | None:
    """Enqueue a machine-scoped control message.

    Routes through the machine outbox: live stream when the dev machine
    has an active control session, offline buffer when it does not.
    Offline-rejected types (``create_runner`` / ``config_push``) raise
    :class:`MachineOfflineError`.

    Unlike the fire-and-forget :func:`send_to_runner`, delivery failures
    are NOT swallowed: machine commands are user-initiated from the web
    UI, and a silent drop would leave the operator polling a "pending"
    result to timeout. Redis errors propagate; a ``None`` return means
    the message was not written to the live stream (Redis unavailable —
    for offline-reject types nothing was delivered at all).
    """
    return enqueue_for_machine(dev_machine_id, _ensure_envelope(message))


def close_runner_session(runner_id: UUID | str, code: int = 4010) -> None:
    """Tell the cloud to evict any active session for this runner.

    Implemented as a session row revoke + Redis pub/sub eviction signal.
    See ``design.md`` §7.6.
    """
    from django.utils import timezone

    from pi_dash.runner.models import RunnerSession
    from pi_dash.runner.services.outbox import (
        clear_session_marker,
        publish_session_eviction,
    )

    sessions = list(
        RunnerSession.objects.filter(
            runner_id=runner_id, revoked_at__isnull=True
        )
    )
    for session in sessions:
        session.revoked_at = timezone.now()
        session.revoked_reason = "force_close"
        session.save(update_fields=["revoked_at", "revoked_reason"])
        clear_session_marker(session.id)
        publish_session_eviction(
            runner_id, old_session_id=str(session.id), new_session_id=""
        )


def send_runner_revoke(
    runner_id: UUID | str, reason: str = "runner revoked"
) -> None:
    """Enqueue a ``revoke`` control frame for the runner.

    Replaces the old ``send_connection_revoke`` channels-layer fan-out.
    """
    try:
        enqueue_for_runner(
            runner_id,
            {"type": "revoke", "reason": reason},
        )
    except RunnerOfflineError:
        # ``revoke`` is in the offline-allowed set; it cannot raise this
        # error in practice. Log defensively if it ever does.
        logger.warning("revoke enqueue rejected as offline for %s", runner_id)
    except Exception:
        logger.exception("send_runner_revoke failed for %s", runner_id)


def send_runner_remove(
    runner_id: UUID | str, reason: str = "deleted by user"
) -> None:
    """Enqueue a ``remove_runner`` control frame for the runner.

    The daemon's ``RunnerLoop`` handler (Rust:
    ``ServerMsg::RemoveRunner``) responds by:

    - cancelling the in-flight run (if any),
    - removing the runner from its live mailbox + hello maps,
    - deleting the per-runner data dir on disk, and
    - stripping the matching ``[[runner]]`` block from
      ``config.toml`` under the host-wide config lock.

    The systemd unit (``pidash.service``) hosts every runner under one
    process, so the daemon never touches it on this path; only
    ``pidash uninstall`` removes the service.

    Use this verb when the user opted into a *cascade* delete (the
    "Also delete the local runner instance" checkbox in the web UI, or
    the default for ``pidash runner remove`` when the cloud is
    reachable). For a cloud-only delete that leaves the local install
    in place, call :func:`send_runner_revoke` instead.
    """
    try:
        enqueue_for_runner(
            runner_id,
            {
                "type": "remove_runner",
                "runner_id": str(runner_id),
                "reason": reason,
            },
        )
    except RunnerOfflineError:
        # ``remove_runner`` is in the offline-allowed set, but a runner
        # that is currently offline can't act on the frame anyway —
        # log defensively if the queue ever rejects it.
        logger.warning(
            "remove_runner enqueue rejected as offline for %s", runner_id
        )
    except Exception:
        logger.exception("send_runner_remove failed for %s", runner_id)


# Backwards-compatibility alias for any caller still importing the old
# name. New code should call ``send_runner_revoke`` directly.
send_connection_revoke = send_runner_revoke
