"""Fail-closed creation-rate admission for the shared Cloud Agent."""

from __future__ import annotations

import logging
import time

from django.conf import settings
from django.core.cache import cache
from django.db import transaction

logger = logging.getLogger(__name__)


class CloudAgentAdmissionError(RuntimeError):
    def __init__(self, code: str, detail: str, *, retry_after_seconds: int | None = None):
        super().__init__(detail)
        self.code = code
        self.retry_after_seconds = retry_after_seconds


def _consume(key: str, retry_after_seconds: int) -> None:
    try:
        if not cache.add(key, 1, timeout=retry_after_seconds + 5):
            cache.incr(key)
    except Exception:
        # Best effort — the check in _take already failed closed on cache
        # errors; a lost increment only under-counts one bucket.
        logger.exception("cloud-agent admission: failed to consume token %s", key)


def _take(key: str, limit: int, retry_after_seconds: int) -> None:
    """Reject on the bucket's current count; consume a token only on commit.

    The token consumption is deferred to ``transaction.on_commit`` so a
    creation attempt whose surrounding transaction rolls back (prompt too
    large, admission refused downstream, any later validation error) does not
    burn quota — repeated rejections must not lock the user out. Outside an
    atomic block ``on_commit`` runs immediately, preserving the plain-call
    behavior. The check-then-increment window lets a concurrent burst
    slightly overshoot the per-minute rate; the workspace queue cap remains
    the hard limit.
    """
    try:
        count = int(cache.get(key) or 0)
    except Exception as exc:
        raise CloudAgentAdmissionError(
            "admission_unavailable",
            "Cloud Agent admission is temporarily unavailable",
            retry_after_seconds=retry_after_seconds,
        ) from exc
    if count >= limit:
        raise CloudAgentAdmissionError(
            "run_quota_exceeded",
            "Cloud Agent creation rate exceeded",
            retry_after_seconds=retry_after_seconds,
        )
    transaction.on_commit(lambda: _consume(key, retry_after_seconds))


def enforce_creation_rate(*, workspace_id, actor_id=None, automatic: bool = False) -> None:
    """Consume fixed one-minute buckets without making Redis a capacity store."""
    now = int(time.time())
    retry_after = 60 - (now % 60)
    bucket = now // 60
    _take(
        f"cloud-agent:admission:workspace:{workspace_id}:{bucket}",
        settings.CLOUD_AGENT_WORKSPACE_CREATION_RATE_PER_MINUTE,
        retry_after,
    )
    if not automatic and actor_id is not None:
        _take(
            f"cloud-agent:admission:user:{actor_id}:{bucket}",
            settings.CLOUD_AGENT_USER_CREATION_RATE_PER_MINUTE,
            retry_after,
        )
