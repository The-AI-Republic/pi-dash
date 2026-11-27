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


def seed_throttle_window(
    rdb,
    scope: str,
    ident: str,
    *,
    age_secs: float = 0.2,
    count: int = 30,
    window_secs: int = 60,
) -> float:
    """Seed a full throttle window so the next request is denied with a known wait.

    Writes ``count`` identical timestamps ``age_secs`` old, so the true wait
    is ``window_secs - age_secs`` (fractional): DRF answers
    ``Retry-After: ceil(wait)``. Both backend key formats are written — the
    Django ``django_redis`` pickle under the ``:1:`` version prefix and the
    Rust compact-JSON float array — because the two never collide and the
    suite runs against either backend. Returns the seeded timestamp.
    """
    import json
    import pickle
    import time

    t0 = time.time() - age_secs
    history = [t0] * count
    rdb.set(f"throttle_{scope}_{ident}", json.dumps(history), ex=window_secs)
    rdb.set(f":1:throttle_{scope}_{ident}", pickle.dumps(history), ex=window_secs)
    return t0
