# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/set-password/ — DRF APIView, authenticated only.

Only users with ``is_password_autoset`` may set a password; success is
200 with the serialized user. A weak password answers INVALID_PASSWORD
(5020) here — not PASSWORD_TOO_WEAK (5021) as in change-password/.
"""

from _harness.checks import forbid_keys, require_keys

from .conftest import NEW_PASSWORD, WEAK_PASSWORD, login_cookies


def test_autoset_user_success_shape(browser, autoset_user):
    res = browser(login_cookies(autoset_user)).post(
        "/auth/set-password/", json={"password": NEW_PASSWORD}
    )
    assert res.status_code == 200, res.text[:200]
    body = res.json()
    require_keys(body, ["id", "email", "is_password_autoset"], ctx="set-password")
    forbid_keys(body, ["password"], ctx="set-password")
    assert body["id"] == autoset_user["id"]
    assert body["email"] == autoset_user["email"]
    assert body["is_password_autoset"] is False


def test_new_password_signs_in(browser, autoset_user):
    browser(login_cookies(autoset_user)).post(
        "/auth/set-password/", json={"password": NEW_PASSWORD}
    )
    res = browser().post(
        "/auth/sign-in/", data={"email": autoset_user["email"], "password": NEW_PASSWORD}
    )
    assert res.status_code == 302
    assert "error_code" not in res.headers["location"]


def test_already_set_password_is_rejected(browser, user):
    res = browser(login_cookies(user)).post(
        "/auth/set-password/", json={"password": NEW_PASSWORD}
    )
    assert res.status_code == 400
    assert res.json()["error_code"] == 5145
    assert res.json()["error_message"] == "PASSWORD_ALREADY_SET"


def test_missing_password(browser, autoset_user):
    res = browser(login_cookies(autoset_user)).post("/auth/set-password/", json={})
    assert res.status_code == 400
    assert res.json() == {"error_code": 5020, "error_message": "INVALID_PASSWORD"}


def test_weak_password_is_invalid_password(browser, autoset_user):
    res = browser(login_cookies(autoset_user)).post(
        "/auth/set-password/", json={"password": WEAK_PASSWORD}
    )
    assert res.status_code == 400
    assert res.json() == {"error_code": 5020, "error_message": "INVALID_PASSWORD"}


def test_unauthenticated_is_denied(browser):
    res = browser().post("/auth/set-password/", json={"password": NEW_PASSWORD})
    assert res.status_code in (401, 403), res.text[:200]
