# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""csrftoken cookie carries Domain=COOKIE_DOMAIN when configured (PIDASHCONV-1312).

Django's ``_set_csrf_cookie`` always passes ``domain=settings.CSRF_COOKIE_DOMAIN``,
so with ``COOKIE_DOMAIN`` set every csrftoken cookie is Domain-scoped. The
get-csrf-token endpoint and the change/set-password re-login must match.

Run with the backend started with ``COOKIE_DOMAIN`` set and the same value
exported for pytest::

    COOKIE_DOMAIN=.example.test pytest auth_session/test_csrf_cookie_domain.py

Skips when ``COOKIE_DOMAIN`` is unset: a backend without a cookie domain
emits no ``Domain=`` attribute (pinned by the default-env suites), so there
is nothing to assert.
"""

import os

import httpx
import pytest

from _harness.sessions import SESSION_COOKIE_NAME

from .conftest import NEW_PASSWORD, PASSWORD, login_cookies

COOKIE_DOMAIN = os.environ.get("COOKIE_DOMAIN")

pytestmark = pytest.mark.skipif(
    not COOKIE_DOMAIN,
    reason="COOKIE_DOMAIN not set — backend runs without a cookie domain",
)


def _csrftoken_cookie(res):
    found = [c for c in res.headers.get_list("set-cookie") if c.startswith("csrftoken=")]
    assert len(found) == 1, res.headers.get_list("set-cookie")
    return found[0]


def _prime_csrf(client):
    """Fresh (masked token, raw secret) pair; the secret comes from the raw
    Set-Cookie line, not the jar."""
    res = client.get("/auth/get-csrf-token/")
    assert res.status_code == 200, res.text[:200]
    raw = _csrftoken_cookie(res)
    secret = raw.split("=", 1)[1].split(";", 1)[0]
    assert secret, raw
    return res.json()["csrf_token"], secret


def _post_authed(base, session_key, url, payload):
    """POST with session + CSRF cookies sent as an explicit ``Cookie`` header.

    The suite hits the backend at 127.0.0.1, where a cookie jar withholds
    ``Domain=``-scoped cookies (Django then 403s with "CSRF cookie not
    set"), so the jar is bypassed like ``test_duplicate_cookies.py`` does.
    """
    with httpx.Client(base_url=base, timeout=10, follow_redirects=False) as client:
        token, secret = _prime_csrf(client)
        client.cookies.clear()
        return client.post(
            url,
            json=payload,
            headers={
                "Cookie": f"{SESSION_COOKIE_NAME}={session_key}; csrftoken={secret}",
                "X-CSRFToken": token,
            },
        )


def test_get_csrf_token_sets_domain_scoped_cookie(anon):
    res = anon.get("/auth/get-csrf-token/")
    assert res.status_code == 200
    raw = _csrftoken_cookie(res)
    assert f"Domain={COOKIE_DOMAIN}" in raw, raw


def test_change_password_relogin_sets_domain_scoped_cookie(base, user):
    session_key = login_cookies(user)[SESSION_COOKIE_NAME]
    res = _post_authed(
        base,
        session_key,
        "/auth/change-password/",
        {"old_password": PASSWORD, "new_password": NEW_PASSWORD},
    )
    assert res.status_code == 200, res.text[:200]
    raw = _csrftoken_cookie(res)
    assert f"Domain={COOKIE_DOMAIN}" in raw, raw


def test_set_password_relogin_sets_domain_scoped_cookie(base, autoset_user):
    session_key = login_cookies(autoset_user)[SESSION_COOKIE_NAME]
    res = _post_authed(
        base, session_key, "/auth/set-password/", {"password": NEW_PASSWORD}
    )
    assert res.status_code == 200, res.text[:200]
    raw = _csrftoken_cookie(res)
    assert f"Domain={COOKIE_DOMAIN}" in raw, raw
