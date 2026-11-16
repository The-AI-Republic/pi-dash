"""Anonymous throttle state expires one window after the last allowed hit.

Django keeps its throttle history in the default django-redis cache
(``SimpleRateThrottle.throttle_success`` does ``cache.set(key, history,
duration)``), so a ``throttle_anon_<ident>`` key vanishes 60 s after the last
allowed request and that ident starts again with the full quota. Rust keeps
the same histories in a process-local map keyed the same way; PIDASHCONV-1206
fixed a version of that map that never dropped a key, so an anonymous caller
rotating ``X-Forwarded-For`` grew the API process's memory without bound.

A leak is not observable over HTTP — the allow/deny/wait arithmetic for any
single ident is identical on both backends before *and* after the fix — so
this suite pins the *cross-backend contract* the fix must preserve: distinct
idents bucket independently, an exhausted ident is denied with a
``Retry-After``, other idents are unaffected, and after the window passes the
exhausted ident is allowed again with the full 30/minute quota. The
failing-before-fix proof is the Rust unit test
``v1_openapi::tests::store_drops_ident_once_its_window_has_passed`` (plus
``store_sweeps_idents_never_seen_again``), which asserts the store sheds a key
whose window has passed.
"""

from __future__ import annotations

import os
import time
import uuid

import httpx
import pytest

from _harness import env

from . import client as sc

#: The pinned anonymous rate (``settings/common.py``: 30/minute per IP).
QUOTA = 30

#: Window plus slack; the throttled ident must be re-allowed after this.
WINDOW_WAIT = float(os.environ.get("CONTRACT_ANON_WAIT", "75"))


def _ident() -> str:
    """A fresh client ident. ``X-Forwarded-For`` is the throttled ident on
    both backends (``BaseThrottle.get_ident``), so a unique value per test
    gets its own bucket and never collides with the shared ``127.0.0.1``
    bucket other suites drain.
    """
    return f"10.{uuid.uuid4().int % 250 + 1}.{uuid.uuid4().int % 250 + 1}.{uuid.uuid4().int % 250 + 1}"


def _get(ident: str) -> httpx.Response:
    with httpx.Client(base_url=env.BASE_URL, timeout=60.0) as c:
        return c.get(sc.REDOC, headers={"X-Forwarded-For": ident})


@pytest.mark.contract
def test_anon_throttle_state_expires_after_its_window():
    exhausted = _ident()
    other = _ident()

    # A full quota is allowed, then the ident is denied with a Retry-After.
    for n in range(QUOTA):
        r = _get(exhausted)
        assert r.status_code == 200, f"request {n + 1} not allowed: {r.status_code}"
    denied = _get(exhausted)
    assert denied.status_code == 429, f"burst of {QUOTA + 1} never throttled"
    assert int(denied.headers["retry-after"]) >= 1

    # Buckets are per ident: an unrelated ident is untouched.
    assert _get(other).status_code == 200

    # Rotating many distinct idents is all-allowed (one fresh bucket each) —
    # the traffic the Rust map leaked a key for, forever.
    for _ in range(QUOTA * 2):
        assert _get(_ident()).status_code == 200

    # A window after the last allowed hit the state is gone: the exhausted
    # ident is allowed again with the full quota (Django's redis key expired).
    time.sleep(WINDOW_WAIT)
    for n in range(QUOTA):
        r = _get(exhausted)
        assert r.status_code == 200, (
            f"ident not re-allowed with full quota after the window "
            f"(request {n + 1}): {r.status_code} — its throttle state did "
            f"not expire"
        )
