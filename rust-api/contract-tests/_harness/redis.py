# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Redis access for contract suites.

Suites that pin throttled or token flows (magic links, device flow, email
verification) read server-side Redis state straight over ``CONTRACT_REDIS_URL``
(the host-reachable twin of the backend's ``REDIS_URL``) and reset the keys
they touch per test. Nothing here imports Django.
"""

from __future__ import annotations

import json
import os


def redis_url() -> str:
    try:
        return os.environ["CONTRACT_REDIS_URL"]
    except KeyError:
        raise AssertionError(
            "CONTRACT_REDIS_URL is required (host-reachable Redis URL of the "
            "backend under test, e.g. 'redis://127.0.0.1:16301/0')"
        )


def client():
    import redis

    return redis.Redis.from_url(redis_url(), db=0, decode_responses=True)


def magic_key(email: str) -> str:
    return "magic_" + email.strip().lower()


def read_magic(rdb, email: str) -> dict | None:
    raw = rdb.get(magic_key(email))
    if raw is None:
        return None
    return json.loads(raw)


def clear_magic(rdb, email: str) -> None:
    rdb.delete(magic_key(email))


def pause_processing(rdb, pause_ms: int) -> None:
    """Stall the backend's Redis for ``pause_ms`` (``CLIENT PAUSE``).

    Fault injection for timeout pins (PIDASHCONV-1080): while paused the
    server accepts connections but answers no command, so every in-flight
    backend op waits it out or hits its socket timeout. The pause always
    runs its full length — ``CLIENT UNPAUSE`` is itself queued behind it —
    so keep windows short and let them expire naturally.
    """
    rdb.execute_command("CLIENT", "PAUSE", str(int(pause_ms)))


def clear_throttle_counters(rdb) -> None:
    """Delete DRF throttle counters (django-redis ``*throttle*`` keys).

    Suites reset these per test so per-IP rate limits never leak between
    tests. Attempt-count guards stored in ``magic_*`` keys are untouched.
    """
    for key in list(rdb.scan_iter("*throttle*")):
        rdb.delete(key)
