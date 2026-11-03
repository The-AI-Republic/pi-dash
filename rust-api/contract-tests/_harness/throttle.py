# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Throttle-counter reset for suites hitting throttled Views.

``AuthenticationThrottle`` (DRF ``AnonRateThrottle``, 30/minute) counts
per client IP in the default cache, which is redis-backed
(``django_redis`` unconditionally in settings). The suite runs its own
redis db (``REDIS_URL``), so flushing that db resets every throttle
counter without touching anything else. No-op when ``REDIS_URL`` is
unset — but suites that assert on throttled endpoints should require it
(see ``auth_session``) rather than risk 429 flakes.
"""

import os


def reset_throttle_counters() -> None:
    """Flush the suite's redis db, resetting all DRF throttle counters."""
    url = os.environ.get("REDIS_URL", "")
    if not url:
        return
    import redis

    redis.Redis.from_url(url, socket_timeout=5).flushdb()


def reset_throttle_key(key: str) -> None:
    """Delete one DRF throttle counter, leaving every other cache key alone.

    Suites that exhaust a per-user throttle use this for the specific
    ``throttle_<scope>_<ident>`` key so the 120-request burst cannot leak
    into another test or a rerun inside the same window.
    """
    url = os.environ.get("REDIS_URL", "")
    if not url:
        return
    import redis

    redis.Redis.from_url(url, socket_timeout=5).delete(key)
