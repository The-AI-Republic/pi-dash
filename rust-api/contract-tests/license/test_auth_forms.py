# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Form-login Views (sign-in / sign-up / sign-out).

No CSRF token is obtainable over plain HTTP (no browsable renderer, no
exemption), so a token-less form POST hits the custom CSRF_FAILURE_VIEW:
HTTP 200 with the csrf_failure template. That quirky but real wire behavior
is the contract; the redirect branches behind a valid token are unreachable
black-box and therefore not covered here.
"""


def _assert_csrf_failure(res):
    assert res.status_code == 200
    assert "text/html" in res.headers["content-type"]
    assert "CSRF Verification Failed" in res.text
    assert "templates/csrf_failure.html" in res.text


def test_signin_without_csrf_token(anon_api):
    res = anon_api.post(
        "/api/instances/admins/sign-in/",
        data={"email": "admin@example.com", "password": "ContractPass123!"},
    )
    _assert_csrf_failure(res)


def test_signup_without_csrf_token(anon_api):
    res = anon_api.post(
        "/api/instances/admins/sign-up/",
        data={"email": "fresh@example.com", "password": "SomeStrongPass123!", "first_name": "Fresh"},
    )
    _assert_csrf_failure(res)


def test_signout_without_csrf_token(admin_api):
    res = admin_api.post("/api/instances/admins/sign-out/")
    _assert_csrf_failure(res)
