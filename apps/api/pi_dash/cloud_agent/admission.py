"""Fail-closed creation-rate admission for the shared Cloud Agent."""

from __future__ import annotations

import time

from django.conf import settings
from django.core.cache import cache


class CloudAgentAdmissionError(RuntimeError):
    def __init__(self, code: str, detail: str, *, retry_after_seconds: int | None = None):
        super().__init__(detail)
        self.code = code
        self.retry_after_seconds = retry_after_seconds


def _take(key: str, limit: int, retry_after_seconds: int) -> None:
    try:
        if cache.add(key, 1, timeout=retry_after_seconds + 5):
            count = 1
        else:
            count = cache.incr(key)
    except Exception as exc:
        raise CloudAgentAdmissionError(
            "admission_unavailable",
            "Cloud Agent admission is temporarily unavailable",
            retry_after_seconds=retry_after_seconds,
        ) from exc
    if count > limit:
        raise CloudAgentAdmissionError(
            "run_quota_exceeded",
            "Cloud Agent creation rate exceeded",
            retry_after_seconds=retry_after_seconds,
        )


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
