# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: magic Redis stall behavior (PIDASHCONV-1080).

Fault-injection pins of the magic Redis bounds against a live backend
(Django today, Rust through the same suite). ``CLIENT PAUSE`` stalls the
backend's own Redis for a fixed window — no reboot, no re-pointing
``REDIS_URL`` — and the suite asserts the request outcome:

- Long stall (8s) + sign-in -> 500 within 10s on both backends.
  Sign-in is the route where Django's bound holds: it is an
  unthrottled plain ``View`` hitting only ``redis_instance()`` (5s
  socket timeout), so Django 500s in ~5s and Rust in ~1-2s.
  Generate cannot carry this assertion: Django's DRF throttle rides
  the django-redis cache, which configures no socket timeout
  (``common.py`` ``CACHES``), so Django hangs through a long stall
  and 200s after it (measured: 200 in 8.1s vs an 8s stall) while
  Rust 500s bounded. That divergence is the deliberate fail-open
  throttle / 500-on-redis-error precedent, out of scope here.
- Medium stall (3.5s) + generate -> 200 on both backends. Django
  rides it through (~3.6s); pre-fix Rust 500s at ~3s (the ``redis``
  crate's 1s connect / 500ms response defaults fail each op, and the
  main ``GET`` never sees the lift), while fixed Rust honors
  ``REDIS_SOCKET_CONNECT_TIMEOUT``/``REDIS_SOCKET_TIMEOUT`` (2s/5s)
  and 200s. This is the fails-before/passes-after pin for the fix.

The 3.5s window sits mid-quantum on both sides (pre-fix op
deadlines at 1/2/3s, post-fix setup windows at 2/4s), and the pause
lift is a server-side timer, so both assertions have ~0.5s of
deterministic margin. Pauses always run their full length (an
``UNPAUSE`` is queued behind the pause), so the long-stall class
runs last in this file to keep residual stall out of its siblings.
"""

import time

import pytest

from _harness import redis as redis_helper

from .conftest import csrf_client, fresh_email, generate, post_form

APP_GENERATE = "/auth/magic-generate/"
SPACE_GENERATE = "/auth/spaces/magic-generate/"
APP_SIGN_IN = "/auth/magic-sign-in/"
SPACE_SIGN_IN = "/auth/spaces/magic-sign-in/"

# Medium stall both backends ride through (200); pre-fix Rust 500s here.
SLOW_PAUSE_MS = 3500
# Long stall: sign-in must 500 inside the bound on both backends.
STALL_PAUSE_MS = 8000
STALL_BOUND_SECS = 10.0


def _drain_pause(pause_ms: int, started: float) -> None:
    """Sleep until a ``CLIENT PAUSE`` window has fully expired.

    The pause always runs its full length, and the harness redis client
    times out reads at 5s (redis-py default) — so a test that asserts
    early (Rust 500s in ~1-2s of an 8s stall) must not return while
    stalled, or the next test's throttle-reset fixture errors reading
    through the residual pause.
    """
    remaining = pause_ms / 1000.0 - (time.monotonic() - started)
    if remaining > 0:
        time.sleep(remaining + 0.5)


@pytest.mark.contract
@pytest.mark.parametrize("path", [APP_GENERATE, SPACE_GENERATE])
class TestSlowRedisGenerate:
    def test_generate_rides_through_medium_stall(self, path, rdb, seed):
        client = csrf_client()
        email = fresh_email(prefix="ct1080slow")
        redis_helper.clear_magic(rdb, email)

        redis_helper.pause_processing(rdb, SLOW_PAUSE_MS)
        started = time.monotonic()
        try:
            resp = generate(client, path, email)
            elapsed = time.monotonic() - started

            assert resp.status_code == 200, (resp.status_code, resp.text[:200], elapsed)
            assert resp.json() == {"key": "magic_" + email}
        finally:
            _drain_pause(SLOW_PAUSE_MS, started)


@pytest.mark.contract
@pytest.mark.parametrize("path", [APP_SIGN_IN, SPACE_SIGN_IN])
class TestStalledRedisSignIn:
    def test_signin_500s_within_bound(self, path, rdb, seed):
        user = seed.user(email=fresh_email(prefix="ct1080stall"))
        client = csrf_client()

        redis_helper.pause_processing(rdb, STALL_PAUSE_MS)
        started = time.monotonic()
        try:
            resp = post_form(client, path, {"email": user["email"], "code": "000000"})
            elapsed = time.monotonic() - started

            assert resp.status_code == 500, (resp.status_code, resp.text[:200], elapsed)
            assert elapsed < STALL_BOUND_SECS, elapsed
        finally:
            _drain_pause(STALL_PAUSE_MS, started)
