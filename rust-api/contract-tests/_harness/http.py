# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""HTTP client factories for contract suites."""

import httpx

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


# httpx client factories for the HTTP suites (space first use; extended by
# dispatch, PIDASHCONV-22). No Django test client anywhere near here.
def anonymous_client(base_url: str, timeout: float = 30.0) -> httpx.Client:
    """Unauthenticated client: the denied-permission and public-shape cases."""
    return httpx.Client(base_url=base_url, timeout=timeout)


def api_client(base_url: str, timeout: float = 30.0, **kwargs) -> httpx.Client:
    """Authenticated-capable client with a cookie jar.

    Sessions come from ``login_session`` (a black-box sign-in POST), so the
    jar here is what carries the session cookie afterwards.
    """
    return httpx.Client(base_url=base_url, timeout=timeout, follow_redirects=True, **kwargs)
