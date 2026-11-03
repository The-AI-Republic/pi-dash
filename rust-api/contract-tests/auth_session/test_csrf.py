# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GET /auth/get-csrf-token/ — DRF APIView, AllowAny, 200 {"csrf_token"}."""

from _harness.checks import require_keys


def test_shape(anon):
    res = anon.get("/auth/get-csrf-token/")
    assert res.status_code == 200
    body = res.json()
    require_keys(body, ["csrf_token"], ctx="get-csrf-token")
    assert isinstance(body["csrf_token"], str) and body["csrf_token"], body


def test_sets_csrf_cookie(anon):
    res = anon.get("/auth/get-csrf-token/")
    assert res.status_code == 200
    assert "csrftoken" in res.cookies, dict(res.cookies)


def test_token_is_usable_for_form_posts(browser, base, user):
    # The primed browser client (used by every form-POST test here) proves
    # the token end to end: without a working token these POSTs would 200
    # on the CSRF failure page instead of 302.
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": user["password"]})
    assert res.status_code == 302
    assert res.headers["location"] == base
