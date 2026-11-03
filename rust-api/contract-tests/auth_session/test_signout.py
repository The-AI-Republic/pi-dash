# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/sign-out/ + /auth/spaces/sign-out/ — form Views, always 302."""

import httpx

from _harness.redirects import location_of

from .conftest import reprime


def test_sign_out_redirects_to_app_base(browser, base, user):
    client = browser()
    res = client.post("/auth/sign-in/", data={"email": user["email"], "password": user["password"]})
    assert res.status_code == 302
    reprime(client)
    res = client.post("/auth/sign-out/")
    assert location_of(res) == base


def test_sign_out_kills_the_session(browser, base, user):
    client = browser()
    client.post("/auth/sign-in/", data={"email": user["email"], "password": user["password"]})
    reprime(client)
    client.post("/auth/sign-out/")
    # The signed-out cookie no longer authenticates: a session-gated View
    # answers 401 instead of reaching the password check.
    res = client.post("/auth/change-password/", json={"old_password": "x", "new_password": "y"})
    assert res.status_code == 401


def test_sign_out_without_session_still_redirects(browser, base):
    res = browser().post("/auth/sign-out/")
    assert location_of(res) == base


def test_space_sign_out_redirects_to_space_base(browser, space_base, user):
    client = browser()
    res = client.post(
        "/auth/spaces/sign-in/", data={"email": user["email"], "password": user["password"]}
    )
    assert res.status_code == 302
    reprime(client)
    res = client.post("/auth/spaces/sign-out/")
    assert location_of(res) == space_base


def test_sign_out_without_csrf_token_hits_failure_page(base):
    with httpx.Client(base_url=base, timeout=10, follow_redirects=False) as client:
        res = client.post("/auth/sign-out/")
    assert res.status_code == 200
    assert "CSRF Verification Failed" in res.text
