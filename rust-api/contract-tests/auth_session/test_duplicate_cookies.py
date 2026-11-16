# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Duplicate cookies: the last value wins (Django ``parse_cookie`` parity).

Browsers send two cookies with the same name when a host-only cookie and a
``Domain=``-scoped cookie with that name coexist (PIDASHCONV-1310).
Django's ``parse_cookie`` overwrites earlier duplicates, so the last value
wins; ``pidash_auth::session::cookie_value`` must do the same for both the
``session-id`` and ``csrftoken`` reads.
"""

import urllib.parse

import httpx

from _harness.redirects import assert_error_redirect

from .conftest import _unique_email, login_cookies, make_user


def test_duplicate_session_id_last_wins(db, anon):
    user_a = make_user(db, _unique_email())
    user_b = make_user(db, _unique_email())
    key_a = login_cookies(user_a)["session-id"]
    key_b = login_cookies(user_b)["session-id"]
    assert key_a != key_b
    res = anon.get(
        "/api/users/me/",
        headers={"Cookie": f"session-id={key_a}; session-id={key_b}"},
    )
    assert res.status_code == 200
    assert res.json()["id"] == user_b["id"]
    assert res.json()["email"] == user_b["email"]


def test_duplicate_csrftoken_last_wins(base):
    # Two secrets, oldest first like a browser sends them; the token minted
    # from the second secret must validate.
    first = httpx.Client(base_url=base, timeout=10, follow_redirects=False)
    second = httpx.Client(base_url=base, timeout=10, follow_redirects=False)
    try:
        first.get("/auth/get-csrf-token/").raise_for_status()
        secret1 = first.cookies.get("csrftoken")
        token2 = second.get("/auth/get-csrf-token/").json()["csrf_token"]
        secret2 = second.cookies.get("csrftoken")
    finally:
        first.close()
        second.close()
    assert secret1 and secret2 and secret1 != secret2
    body = urllib.parse.urlencode({"email": f"{_unique_email()}", "password": "x"})
    res = httpx.post(
        f"{base}/auth/sign-in/",
        content=body,
        headers={
            "Cookie": f"csrftoken={secret1}; csrftoken={secret2}",
            "X-CSRFToken": token2,
            "Referer": f"{base}/",
            "Content-Type": "application/x-www-form-urlencoded",
        },
        follow_redirects=False,
        timeout=10,
    )
    # CSRF passes, so the View runs and rejects the unknown email — instead
    # of the 200 CSRF-failure page a first-wins read produces.
    assert "CSRF Verification Failed" not in res.text
    assert_error_redirect(res, base=base, error_code=5060)
