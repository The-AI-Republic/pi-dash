# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-runner Redis Streams outbox.

See ``.ai_design/move_to_https/design.md`` §7.4 / §7.10. Layout:

- ``runner_stream:{rid}`` — persistent live stream (XADD targets)
- ``runner-group:{rid}`` — single consumer group on that stream
- ``consumer-{sid}`` — one consumer name per session
- ``runner_offline_stream:{rid}`` — bounded offline buffer for control
  messages enqueued while the runner has no active session
- ``session_eviction:{rid}`` — pub/sub channel for session evictions

This module exposes the verbs the cloud uses on the outbox: enqueue,
read-for-session, ack, drain-offline, mark-pel-drained, and the helpers
the sweeper uses to trim safely.
"""

from __future__ import annotations

import json
import logging
import uuid as _uuid
from typing import Any, Dict, Iterable, List, Optional, Tuple
from uuid import UUID

from django.conf import settings

from pi_dash.settings.redis import redis_instance

logger = logging.getLogger(__name__)


# Live message types. Anything not in this set is rejected by
# enqueue_for_runner.
_VALID_TYPES = frozenset({
    "assign",
    "cancel",
    "decide",
    "config_push",
    "revoke",
    "remove_runner",
    "resume_ack",
    "force_refresh",
    "welcome",
})

# Offline-allowed types. Per ``design.md`` §7.4: assign/cancel/decide/
# resume_ack get rejected (RunnerOfflineError); the rest queue.
_OFFLINE_REJECT = frozenset({"assign", "cancel", "decide", "resume_ack"})


class RunnerOfflineError(Exception):
    """Raised when a non-queueable control message is enqueued for an
    offline runner.

    Carries the runner id so the dispatcher (matcher / orchestrator) can
    re-queue the corresponding domain row.
    """

    def __init__(self, runner_id: UUID | str, message_type: str) -> None:
        super().__init__(
            f"runner {runner_id} is offline; type {message_type!r} cannot queue"
        )
        self.runner_id = str(runner_id)
        self.message_type = message_type


# ---- Key builders ----------------------------------------------------------


def stream_key(runner_id: UUID | str) -> str:
    return f"runner_stream:{runner_id}"


def group_name(runner_id: UUID | str) -> str:
    return f"runner-group:{runner_id}"


def consumer_name(session_id: UUID | str) -> str:
    return f"consumer-{session_id}"


def offline_stream_key(runner_id: UUID | str) -> str:
    return f"runner_offline_stream:{runner_id}"


def session_eviction_channel(runner_id: UUID | str) -> str:
    return f"session_eviction:{runner_id}"


def session_pel_drained_key(session_id: UUID | str) -> str:
    return f"session_pel_drained:{session_id}"


def stream_cleanup_zset_key() -> str:
    return "runner_stream_cleanup"


# ---- Helpers ---------------------------------------------------------------


def _serialize(message: Dict[str, Any]) -> Dict[str, str]:
    """Encode a control message into the {field: stringified} shape that
    XADD wants.

    The whole payload is stored under ``payload`` JSON; ``mid`` and
    ``type`` are surfaced as separate fields so Lua / inspectors can
    cheaply route without decoding.
    """
    mid = str(message.get("mid") or _uuid.uuid4())
    body = dict(message)
    body["mid"] = mid
    msg_type = str(body.get("type") or "")
    return {
        "mid": mid,
        "type": msg_type,
        "payload": json.dumps(body, default=str),
    }


def _ensure_group(client, runner_id: UUID | str) -> None:
    """Create the persistent stream + consumer group if missing."""
    sk = stream_key(runner_id)
    gn = group_name(runner_id)
    try:
        client.xgroup_create(name=sk, groupname=gn, id="$", mkstream=True)
    except Exception as exc:
        # BUSYGROUP means the group exists; that's fine.
        if "BUSYGROUP" in str(exc):
            return
        raise


def ensure_stream_group(runner_id: UUID | str) -> None:
    """Idempotent ``XGROUP CREATE ... MKSTREAM`` for the runner stream."""
    client = redis_instance()
    if client is None:
        return
    _ensure_group(client, runner_id)


# ---- Active-session lookup -------------------------------------------------


def active_session_id_for_runner(runner_id: UUID | str) -> Optional[str]:
    """Return the active session id for a runner, or ``None``."""
    from pi_dash.runner.models import RunnerSession

    sid = (
        RunnerSession.objects.filter(
            runner_id=runner_id, revoked_at__isnull=True
        )
        .values_list("id", flat=True)
        .first()
    )
    return str(sid) if sid is not None else None


# ---- Outbox verbs ----------------------------------------------------------


def enqueue_for_runner(
    runner_id: UUID | str, message: Dict[str, Any]
) -> Optional[str]:
    """Enqueue a control message.

    Returns the stream id (e.g. ``"1714080000-0"``) if delivered to the
    live stream, ``None`` if delivered to the offline buffer.

    Raises :class:`RunnerOfflineError` for types that must not be
    queued offline (``assign`` / ``cancel`` / ``decide`` /
    ``resume_ack``).
    """
    msg_type = str(message.get("type") or "")
    if msg_type not in _VALID_TYPES:
        raise ValueError(f"unknown message type {msg_type!r}")
    client = redis_instance()
    if client is None:
        logger.warning("redis unavailable; cannot enqueue for runner %s", runner_id)
        return None

    if active_session_id_for_runner(runner_id) is not None:
        _ensure_group(client, runner_id)
        sid = client.xadd(stream_key(runner_id), _serialize(message))
        return sid.decode() if isinstance(sid, bytes) else str(sid)

    if msg_type in _OFFLINE_REJECT:
        raise RunnerOfflineError(runner_id, msg_type)

    maxlen = int(getattr(settings, "OFFLINE_STREAM_MAXLEN", 1000))
    ttl_secs = int(getattr(settings, "OFFLINE_STREAM_TTL_SECS", 86400))
    key = offline_stream_key(runner_id)
    client.xadd(key, _serialize(message), maxlen=maxlen, approximate=True)
    client.expire(key, ttl_secs)
    return None


def drain_offline_into_live(runner_id: UUID | str) -> int:
    """Move every entry from the offline buffer into the live stream.

    Called by ``POST /sessions/`` once the new session row exists.
    Returns the number of entries moved.
    """
    client = redis_instance()
    if client is None:
        return 0
    okey = offline_stream_key(runner_id)
    entries = client.xrange(okey)
    if not entries:
        return 0
    _ensure_group(client, runner_id)
    sk = stream_key(runner_id)
    moved = 0
    for _, fields in entries:
        # ``fields`` is {bytes:bytes}; XADD accepts that directly.
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


def claim_pending_for_new_session(
    runner_id: UUID | str,
    old_consumer: Optional[str],
    new_consumer: str,
    min_idle_ms: int = 0,
) -> int:
    """Reassign every PEL entry from ``old_consumer`` to ``new_consumer``.

    Paginated XAUTOCLAIM loop (``design.md`` §7.6). Returns the number
    of stream IDs claimed. Ignored when there is no prior consumer.
    """
    if not old_consumer:
        return 0
    client = redis_instance()
    if client is None:
        return 0
    sk = stream_key(runner_id)
    gn = group_name(runner_id)
    cursor = "0-0"
    claimed = 0
    while True:
        try:
            result = client.xautoclaim(
                name=sk,
                groupname=gn,
                consumername=new_consumer,
                min_idle_time=min_idle_ms,
                start_id=cursor,
                count=200,
                justid=True,
            )
        except Exception:
            logger.exception("xautoclaim failed for runner %s", runner_id)
            return claimed
        if not result:
            return claimed
        # xautoclaim returns (next_cursor, claimed_ids[, deleted_ids])
        next_cursor = result[0]
        claimed_ids = result[1] if len(result) > 1 else []
        if isinstance(next_cursor, bytes):
            next_cursor = next_cursor.decode()
        claimed += len(claimed_ids)
        if next_cursor == "0-0" or not claimed_ids:
            return claimed
        cursor = next_cursor


def mark_pel_drained(session_id: UUID | str) -> None:
    """Set the per-session marker; called after the first XREADGROUP 0."""
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
    """Delete the per-session pel-drained marker."""
    client = redis_instance()
    if client is None:
        return
    client.delete(session_pel_drained_key(session_id))


def read_for_session(
    runner_id: UUID | str,
    session_id: UUID | str,
    *,
    block_ms: int,
    count: int = 100,
    use_zero: bool = False,
) -> List[Dict[str, Any]]:
    """One ``XREADGROUP`` against the runner stream.

    ``use_zero=True`` reads ``0`` (the consumer's PEL replay); ``False``
    reads ``>`` (new entries). Returns a list of decoded entries with
    ``stream_id``, ``mid``, ``type``, ``body``.
    """
    client = redis_instance()
    if client is None:
        return []
    _ensure_group(client, runner_id)
    sk = stream_key(runner_id)
    gn = group_name(runner_id)
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
        logger.exception("xreadgroup failed for runner %s", runner_id)
        return []
    out: List[Dict[str, Any]] = []
    if not result:
        return out
    for _, entries in result:
        for stream_id, fields in entries:
            sid = stream_id.decode() if isinstance(stream_id, bytes) else str(stream_id)
            decoded = {}
            for k, v in fields.items():
                key = k.decode() if isinstance(k, bytes) else k
                val = v.decode() if isinstance(v, bytes) else v
                decoded[key] = val
            try:
                body = json.loads(decoded.get("payload") or "{}")
            except (TypeError, ValueError):
                body = {}
            out.append(
                {
                    "stream_id": sid,
                    "mid": decoded.get("mid") or body.get("mid") or "",
                    "type": decoded.get("type") or body.get("type") or "",
                    "body": body,
                }
            )
    return out


def ack_for_session(
    runner_id: UUID | str, stream_ids: Iterable[str]
) -> int:
    """Multi-id ``XACK``; returns count of removed PEL entries."""
    ids = [sid for sid in stream_ids if sid]
    if not ids:
        return 0
    client = redis_instance()
    if client is None:
        return 0
    return int(
        client.xack(stream_key(runner_id), group_name(runner_id), *ids)
    )


# ---- Session-eviction signaling -------------------------------------------


def publish_session_eviction(
    runner_id: UUID | str,
    *,
    old_session_id: Optional[str],
    new_session_id: str,
) -> None:
    client = redis_instance()
    if client is None:
        return
    body = json.dumps(
        {"old_sid": old_session_id, "new_sid": new_session_id}
    )
    client.publish(session_eviction_channel(runner_id), body)


# ---- Sweeper helpers ------------------------------------------------------


def schedule_stream_cleanup_for_runner(runner_id: UUID | str) -> None:
    """Mark a runner stream for delayed cleanup.

    Used by ``Runner.revoke()`` so the sweeper can drop the persistent
    stream after a brief observation window. Stored as a Redis ZSET
    keyed on runner id with the cleanup-not-before timestamp.
    """
    import time

    client = redis_instance()
    if client is None:
        return
    # Default grace = 2 * access_token_ttl so the daemon has time to
    # observe shutdown.
    grace = int(getattr(settings, "ACCESS_TOKEN_TTL_SECS", 3600)) * 2
    when = int(time.time()) + grace
    client.zadd(stream_cleanup_zset_key(), {str(runner_id): when})


def due_runners_for_stream_cleanup() -> List[str]:
    import time

    client = redis_instance()
    if client is None:
        return []
    now = int(time.time())
    members = client.zrangebyscore(stream_cleanup_zset_key(), 0, now)
    return [
        m.decode() if isinstance(m, bytes) else str(m) for m in members or []
    ]


def remove_stream_cleanup_marker(runner_id: UUID | str) -> None:
    client = redis_instance()
    if client is None:
        return
    client.zrem(stream_cleanup_zset_key(), str(runner_id))


def delete_runner_stream(runner_id: UUID | str) -> None:
    client = redis_instance()
    if client is None:
        return
    client.delete(stream_key(runner_id))
    client.delete(offline_stream_key(runner_id))


def safe_trim_runner_stream(
    runner_id: UUID | str, *, time_cutoff_id: str
) -> Optional[int]:
    """PEL+undelivered-aware ``XTRIM MINID`` (``design.md`` §7.10).

    ``time_cutoff_id`` is the time-based floor (e.g. ``id_for_secs_ago(3600)``
    in stream-id form). The sweeper provides it; this helper picks the
    safer of {``min_pending_id - 1``, ``last_delivered_id``} and trims to
    ``min(time_cutoff_id, safe_floor)``.

    Returns the count of removed entries (0 when nothing trimmed) or
    ``None`` if the trim was skipped because the cutoff was non-monotonic.
    """
    client = redis_instance()
    if client is None:
        return None
    sk = stream_key(runner_id)
    gn = group_name(runner_id)
    try:
        groups = client.xinfo_groups(sk)
    except Exception:
        logger.exception("xinfo groups failed for %s", runner_id)
        return None
    last_delivered: Optional[str] = None
    for entry in groups or []:
        # ``entry`` is {name, last-delivered-id, pending, consumers}.
        name = entry.get("name") if isinstance(entry, dict) else entry[1]
        if isinstance(name, bytes):
            name = name.decode()
        if str(name) == gn:
            ld = entry.get("last-delivered-id") if isinstance(entry, dict) else entry[3]
            if isinstance(ld, bytes):
                ld = ld.decode()
            last_delivered = str(ld)
            break
    if last_delivered is None:
        return None

    try:
        pending = client.xpending(sk, gn)
    except Exception:
        logger.exception("xpending failed for %s", runner_id)
        return None

    # ``xpending`` summary form returns {pending: int, min: bytes, max:
    # bytes, consumers: [...]}; the redis-py shape can vary so be lenient.
    min_pending: Optional[str] = None
    if isinstance(pending, dict):
        min_id = pending.get("min")
        if isinstance(min_id, bytes):
            min_id = min_id.decode()
        if min_id:
            min_pending = str(min_id)
    elif isinstance(pending, (list, tuple)) and len(pending) >= 2:
        min_id = pending[1]
        if isinstance(min_id, bytes):
            min_id = min_id.decode()
        if min_id:
            min_pending = str(min_id)

    if min_pending is not None:
        safe_floor = _decrement_stream_id(min_pending)
    else:
        safe_floor = last_delivered

    safe_cutoff = _min_stream_id(time_cutoff_id, safe_floor)
    if safe_cutoff is None:
        return None
    try:
        return int(client.xtrim(sk, minid=safe_cutoff))
    except Exception:
        logger.exception("xtrim failed for %s", runner_id)
        return None


# ---- Stream-id arithmetic --------------------------------------------------


def _split_id(sid: str) -> Optional[Tuple[int, int]]:
    if not sid or "-" not in sid:
        return None
    try:
        ms_str, seq_str = sid.split("-", 1)
        return int(ms_str), int(seq_str)
    except (TypeError, ValueError):
        return None


def _format_id(ms: int, seq: int) -> str:
    return f"{ms}-{seq}"


def _min_stream_id(a: Optional[str], b: Optional[str]) -> Optional[str]:
    if a is None:
        return b
    if b is None:
        return a
    pa = _split_id(a)
    pb = _split_id(b)
    if pa is None or pb is None:
        return None
    return _format_id(*pa) if pa <= pb else _format_id(*pb)


def _decrement_stream_id(sid: str) -> str:
    """Return the largest stream id strictly less than ``sid``.

    Stream ids look like ``"1714080000-0"``. Decrementing ``"…-0"``
    rolls the ms portion down by 1 and seq to a large sentinel; we
    only need a value safe for ``MINID`` so plain integer arithmetic
    is fine.
    """
    parts = _split_id(sid)
    if parts is None:
        return sid
    ms, seq = parts
    if seq > 0:
        return _format_id(ms, seq - 1)
    # seq == 0: the next-smaller id is (ms-1)-0 (Redis treats absent
    # seq as 0 for trimming).
    return _format_id(max(0, ms - 1), 0)


def id_for_secs_ago(secs: int) -> str:
    """Return a synthetic stream id whose ms portion is ``now - secs * 1000``."""
    import time

    ms = max(0, int(time.time() * 1000) - secs * 1000)
    return _format_id(ms, 0)
