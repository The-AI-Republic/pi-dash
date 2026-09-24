# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""HTTP client factories for contract suites."""

import httpx
import time

from . import config
from . import signing
from .settings import base_url


def make_client(cookies: dict | None = None) -> httpx.Client:
    """Generic client against ``BASE_URL`` (from ``_harness.config``)."""
    return httpx.Client(base_url=config.base_url(), cookies=cookies, timeout=30.0)


def anon_client(**kwargs):
    return httpx.Client(base_url=base_url(), timeout=15, **kwargs)


def admin_client(session_key, **kwargs):
    return httpx.Client(
        base_url=base_url(),
        cookies={signing.ADMIN_SESSION_COOKIE: session_key},
        timeout=15,
        **kwargs,
    )


# The stock `anon` throttle is 30/min and only unauthenticated traffic counts
# against it — but the pre-login CSRF/sign-in hits plus the anon-client cases
# still burst past it over a full-file run. A 429 is the server asking us to
# wait, never a contract answer (no test pins a throttle shape), so ride it
# out with a bounded retry instead of failing the run.
_MAX_429_ATTEMPTS = 8


class ContractClient(httpx.Client):
    """httpx client that retries 429s, honoring Retry-After."""

    def request(self, method, url, **kwargs):  # type: ignore[override]
        response = super().request(method, url, **kwargs)
        for attempt in range(_MAX_429_ATTEMPTS - 1):
            if response.status_code != 429:
                return response
            retry_after = response.headers.get("retry-after")
            try:
                delay = max(float(retry_after), 1.0)  # type: ignore[arg-type]
            except (TypeError, ValueError):
                delay = 1.0 + attempt
            time.sleep(min(delay, 60.0))
            response = super().request(method, url, **kwargs)
        return response


# httpx client factories for the HTTP suites (space first use; extended by
# dispatch, PIDASHCONV-22). No Django test client anywhere near here.
def anonymous_client(base_url: str, timeout: float = 30.0) -> httpx.Client:
    """Unauthenticated client: the denied-permission and public-shape cases."""
    return ContractClient(base_url=base_url, timeout=timeout)


def api_client(base_url: str, timeout: float = 30.0, **kwargs) -> httpx.Client:
    """Authenticated-capable client with a cookie jar.

    Sessions come from ``login_session`` (a black-box sign-in POST), so the
    jar here is what carries the session cookie afterwards.
    """
    return ContractClient(base_url=base_url, timeout=timeout, follow_redirects=True, **kwargs)


# --- D-19 (PIDASHCONV-77) API-key helpers ---
# Union with the baseline above: ``base_url`` is the baseline's
# ``settings.base_url`` (identical semantics); the X-Api-Key client
# and asserting verbs below are added verbatim.

def client(api_key=None):
    headers = {}
    if api_key:
        headers["X-Api-Key"] = api_key
    return httpx.Client(base_url=base_url(), headers=headers, timeout=30)


def get(api_key, path, *, expect=200, params=None):
    with client(api_key) as c:
        r = c.get(path, params=params)
    assert r.status_code == expect, f"GET {path}: want {expect}, got {r.status_code}: {r.text[:400]!r}"
    return r


def post(api_key, path, *, expect=201, json=None):
    with client(api_key) as c:
        r = c.post(path, json=json)
    assert r.status_code == expect, f"POST {path}: want {expect}, got {r.status_code}: {r.text[:400]!r}"
    return r


def patch(api_key, path, *, expect=200, json=None):
    with client(api_key) as c:
        r = c.patch(path, json=json)
    assert r.status_code == expect, f"PATCH {path}: want {expect}, got {r.status_code}: {r.text[:400]!r}"
    return r


def delete(api_key, path, *, expect=204):
    with client(api_key) as c:
        r = c.delete(path)
    assert r.status_code == expect, (
        f"DELETE {path}: want {expect}, got {r.status_code}: {r.text[:400]!r}"
    )
    return r

