# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-dev-machine Redis Streams outbox.

The machine-level twin of :mod:`pi_dash.runner.services.outbox`. Where
that module keys every stream on a ``runner_id``, this one keys on a
``dev_machine_id`` so the cloud can push *machine-scoped* control
messages (``create_runner``, ``config_push``, …) down the always-on
machine control session — including to a machine that hosts zero
runners.

Layout mirrors the per-runner outbox:

- ``machine_stream:{mid}`` — persistent live stream (XADD targets)
- ``machine-group:{mid}`` — single consumer group on that stream
- ``machine-consumer-{sid}`` — one consumer name per session
- ``machine_offline_stream:{mid}`` — bounded offline buffer for control
  messages enqueued while the machine has no active session
- ``machine_session_eviction:{mid}`` — pub/sub channel for evictions

The scope-agnostic encode/decode helpers are reused from
:mod:`pi_dash.runner.services.outbox` so the two outboxes stay wire
compatible.
"""

from __future__ import annotations

import logging
from typing import Any, Dict, Iterable, List, Optional
from uuid import UUID

from django.conf import settings

from pi_dash.runner.services.outbox import (
    _decode_read_result,
    _serialize,
)
from pi_dash.settings.redis import redis_instance

logger = logging.getLogger(__name__)


# Live machine-scoped message types. Anything not in this set is rejected
# by :func:`enqueue_for_machine`. Kept deliberately small; new
# machine-level commands opt in here.
_VALID_TYPES = frozenset({
    "welcome",
    "ping",
    "create_runner",
    "config_push",
})

# Types that must never sit in the offline buffer: a machine that is
# not currently polling cannot act on them, and re-delivering a stale
# command later would be surprising. ``create_runner`` is user-initiated
# from the cloud UI and should fail fast (surfaced to the operator) when
# the target machine is offline, rather than silently firing minutes
# later. ``config_push`` follows the same rule.
_OFFLINE_REJECT = frozenset({
    "create_runner",
    "config_push",
})


class MachineOfflineError(Exception):
    """Raised when a non-queueable control message is enqueued for a
    dev machine with no active session."""

    def __init__(self, dev_machine_id: UUID | str, message_type: str) -> None:
        super().__init__(
            f"dev machine {dev_machine_id} is offline; type "
            f"{message_type!r} cannot queue"
        )
        self.dev_machine_id = str(dev_machine_id)
        self.message_type = message_type


# ---- Key builders ----------------------------------------------------------


def stream_key(dev_machine_id: UUID | str) -> str:
    return f"machine_stream:{dev_machine_id}"


def group_name(dev_machine_id: UUID | str) -> str:
    return f"machine-group:{dev_machine_id}"


def consumer_name(session_id: UUID | str) -> str:
    return f"machine-consumer-{session_id}"


def offline_stream_key(dev_machine_id: UUID | str) -> str:
    return f"machine_offline_stream:{dev_machine_id}"


def session_eviction_channel(dev_machine_id: UUID | str) -> str:
    return f"machine_session_eviction:{dev_machine_id}"


def session_pel_drained_key(session_id: UUID | str) -> str:
    return f"machine_session_pel_drained:{session_id}"


# ---- Helpers ---------------------------------------------------------------


def _ensure_group(client, dev_machine_id: UUID | str) -> None:
    sk = stream_key(dev_machine_id)
    gn = group_name(dev_machine_id)
    try:
        client.xgroup_create(name=sk, groupname=gn, id="$", mkstream=True)
    except Exception as exc:
        if "BUSYGROUP" in str(exc):
            return
        raise


async def _aensure_group(client, dev_machine_id: UUID | str) -> None:
    sk = stream_key(dev_machine_id)
    gn = group_name(dev_machine_id)
    try:
        await client.xgroup_create(name=sk, groupname=gn, id="$", mkstream=True)
    except Exception as exc:
        if "BUSYGROUP" in str(exc):
            return
        raise


def ensure_stream_group(dev_machine_id: UUID | str) -> None:
    """Idempotent ``XGROUP CREATE ... MKSTREAM`` for the machine stream."""
    client = redis_instance()
    if client is None:
        return
    _ensure_group(client, dev_machine_id)


# ---- Active-session lookup -------------------------------------------------


def active_session_id_for_machine(dev_machine_id: UUID | str) -> Optional[str]:
    """Return the active machine session id, or ``None``."""
    from pi_dash.runner.models import MachineSession

    sid = (
        MachineSession.objects.filter(
            dev_machine_id=dev_machine_id, revoked_at__isnull=True
        )
        .values_list("id", flat=True)
        .first()
    )
    return str(sid) if sid is not None else None


# ---- Outbox verbs ----------------------------------------------------------


def enqueue_for_machine(
    dev_machine_id: UUID | str, message: Dict[str, Any]
) -> Optional[str]:
    """Enqueue a machine-scoped control message.

    Returns the stream id if delivered to the live stream, ``None`` if
    delivered to the offline buffer. Raises :class:`MachineOfflineError`
    for types that must not queue offline.
    """
    msg_type = str(message.get("type") or "")
    if msg_type not in _VALID_TYPES:
        raise ValueError(f"unknown machine message type {msg_type!r}")
    client = redis_instance()
    if client is None:
        logger.warning(
            "redis unavailable; cannot enqueue for dev machine %s", dev_machine_id
        )
        return None

    if active_session_id_for_machine(dev_machine_id) is not None:
        _ensure_group(client, dev_machine_id)
        sid = client.xadd(stream_key(dev_machine_id), _serialize(message))
        return sid.decode() if isinstance(sid, bytes) else str(sid)

    if msg_type in _OFFLINE_REJECT:
        raise MachineOfflineError(dev_machine_id, msg_type)

    maxlen = int(getattr(settings, "OFFLINE_STREAM_MAXLEN", 1000))
    ttl_secs = int(getattr(settings, "OFFLINE_STREAM_TTL_SECS", 86400))
    key = offline_stream_key(dev_machine_id)
    client.xadd(key, _serialize(message), maxlen=maxlen, approximate=True)
    client.expire(key, ttl_secs)
    return None


def drain_offline_into_live(dev_machine_id: UUID | str) -> int:
    """Move every entry from the offline buffer into the live stream.

    Called by session open once the new session row exists. Returns the
    number of entries moved.
    """
    client = redis_instance()
    if client is None:
        return 0
    okey = offline_stream_key(dev_machine_id)
    entries = client.xrange(okey)
    if not entries:
        return 0
    _ensure_group(client, dev_machine_id)
    sk = stream_key(dev_machine_id)
    moved = 0
    for _, fields in entries:
        decoded = {
            (k.decode() if isinstance(k, bytes) else k): (
                v.decode() if isinstance(v, bytes) else v
            )
            for k, v in fields.items()
        }
        client.xadd(sk, decoded)
        moved += 1
    client.delete(okey)
    return moved


def mark_pel_drained(session_id: UUID | str) -> None:
    client = redis_instance()
    if client is None:
        return
    ttl = int(getattr(settings, "ACCESS_TOKEN_TTL_SECS", 3600)) * 2
    client.set(session_pel_drained_key(session_id), "1", ex=ttl)


def is_pel_drained(session_id: UUID | str) -> bool:
    client = redis_instance()
    if client is None:
        return False
    return bool(client.exists(session_pel_drained_key(session_id)))


def clear_session_marker(session_id: UUID | str) -> None:
    client = redis_instance()
    if client is None:
        return
    client.delete(session_pel_drained_key(session_id))


def read_for_session(
    dev_machine_id: UUID | str,
    session_id: UUID | str,
    *,
    block_ms: int,
    count: int = 100,
    use_zero: bool = False,
) -> List[Dict[str, Any]]:
    """One ``XREADGROUP`` against the machine stream."""
    client = redis_instance()
    if client is None:
        return []
    _ensure_group(client, dev_machine_id)
    sk = stream_key(dev_machine_id)
    gn = group_name(dev_machine_id)
    cn = consumer_name(session_id)
    last_id = "0" if use_zero else ">"
    try:
        result = client.xreadgroup(
            groupname=gn,
            consumername=cn,
            streams={sk: last_id},
            count=count,
            block=block_ms,
        )
    except Exception:
        logger.exception("xreadgroup failed for dev machine %s", dev_machine_id)
        return []
    return _decode_read_result(result)


async def aread_for_session(
    dev_machine_id: UUID | str,
    session_id: UUID | str,
    *,
    block_ms: int,
    count: int = 100,
    use_zero: bool = False,
) -> List[Dict[str, Any]]:
    """Async twin of :func:`read_for_session`."""
    from pi_dash.settings.redis import async_redis_instance

    client = async_redis_instance()
    if client is None:
        return []
    await _aensure_group(client, dev_machine_id)
    sk = stream_key(dev_machine_id)
    gn = group_name(dev_machine_id)
    cn = consumer_name(session_id)
    last_id = "0" if use_zero else ">"
    try:
        result = await client.xreadgroup(
            groupname=gn,
            consumername=cn,
            streams={sk: last_id},
            count=count,
            block=block_ms,
        )
    except Exception:
        logger.exception("xreadgroup failed for dev machine %s", dev_machine_id)
        return []
    return _decode_read_result(result)


def ack_for_session(
    dev_machine_id: UUID | str, stream_ids: Iterable[str]
) -> int:
    """Multi-id ``XACK``; returns count of removed PEL entries."""
    ids = [sid for sid in stream_ids if sid]
    if not ids:
        return 0
    client = redis_instance()
    if client is None:
        return 0
    return int(
        client.xack(stream_key(dev_machine_id), group_name(dev_machine_id), *ids)
    )


# ---- Session-eviction signaling -------------------------------------------


def publish_session_eviction(
    dev_machine_id: UUID | str,
    *,
    old_session_id: Optional[str],
    new_session_id: str,
) -> None:
    import json

    client = redis_instance()
    if client is None:
        return
    body = json.dumps({"old_sid": old_session_id, "new_sid": new_session_id})
    client.publish(session_eviction_channel(dev_machine_id), body)


def delete_machine_stream(dev_machine_id: UUID | str) -> None:
    client = redis_instance()
    if client is None:
        return
    client.delete(stream_key(dev_machine_id))
    client.delete(offline_stream_key(dev_machine_id))
