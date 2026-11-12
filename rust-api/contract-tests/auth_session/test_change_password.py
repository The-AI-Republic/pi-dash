# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/change-password/ — DRF APIView, authenticated only.

Success is 200 ``{"message": "Password updated successfully"}``. The
denied case (no session cookie → 401, and no session cookie is created)
is the suite's permission-floor pin: removing the auth requirement turns
it red (demonstrated in the PR with a reverted one-line patch).
"""

from _harness.checks import require_keys
from _harness.sessions import SESSION_COOKIE_NAME

from .conftest import NEW_PASSWORD, PASSWORD, WEAK_PASSWORD, login_cookies


def _authed(browser, user):
    return browser(login_cookies(user))


def test_success(browser, user):
    res = _authed(browser, user).post(
        "/auth/change-password/",
        json={"old_password": PASSWORD, "new_password": NEW_PASSWORD},
    )
    assert res.status_code == 200, res.text[:200]
    assert res.json() == {"message": "Password updated successfully"}


def test_new_password_signs_in(browser, base, user):
    _authed(browser, user).post(
        "/auth/change-password/",
        json={"old_password": PASSWORD, "new_password": NEW_PASSWORD},
    )
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": NEW_PASSWORD})
    assert res.status_code == 302
    assert "error_code" not in res.headers["location"]


def test_old_password_no_longer_works(browser, base, user):
    _authed(browser, user).post(
        "/auth/change-password/",
        json={"old_password": PASSWORD, "new_password": NEW_PASSWORD},
    )
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": PASSWORD})
    assert res.status_code == 302
    assert "error_code=5065" in res.headers["location"]


def test_wrong_old_password(browser, user):
    res = _authed(browser, user).post(
        "/auth/change-password/",
        json={"old_password": "Wrong-Pass-1!x", "new_password": NEW_PASSWORD},
    )
    assert res.status_code == 400
    assert res.json() == {
        "error_code": 5135,
        "error_message": "INCORRECT_OLD_PASSWORD",
        "error": "Old password is not correct",
    }


def test_missing_old_password(browser, user):
    res = _authed(browser, user).post(
        "/auth/change-password/", json={"new_password": NEW_PASSWORD}
    )
    assert res.status_code == 400
    assert res.json()["error_code"] == 5138
    assert res.json()["error_message"] == "MISSING_PASSWORD"


def test_missing_new_password(browser, user):
    res = _authed(browser, user).post(
        "/auth/change-password/", json={"old_password": PASSWORD}
    )
    assert res.status_code == 400
    assert res.json()["error_code"] == 5138


def test_weak_new_password(browser, user):
    res = _authed(browser, user).post(
        "/auth/change-password/",
        json={"old_password": PASSWORD, "new_password": WEAK_PASSWORD},
    )
    assert res.status_code == 400
    assert res.json() == {"error_code": 5021, "error_message": "PASSWORD_TOO_WEAK"}


def test_autoset_user_needs_no_old_password(browser, autoset_user):
    res = browser(login_cookies(autoset_user)).post(
        "/auth/change-password/", json={"new_password": NEW_PASSWORD}
    )
    assert res.status_code == 200, res.text[:200]
    assert res.json() == {"message": "Password updated successfully"}


def test_unauthenticated_is_denied_and_creates_no_session(browser):
    res = browser().post(
        "/auth/change-password/",
        json={"old_password": "x", "new_password": NEW_PASSWORD},
    )
    assert res.status_code in (401, 403), res.text[:200]
    body = res.json()
    require_keys(body, ["detail"], ctx="change-password denied")
    assert not [
        c for c in res.headers.get_list("set-cookie") if c.startswith(SESSION_COOKIE_NAME + "=")
    ], res.headers.get_list("set-cookie")


def test_success_cycles_session_key_and_deletes_old_row(browser, db, user):
    cookies = login_cookies(user)
    old_key = cookies[SESSION_COOKIE_NAME]
    res = browser(cookies).post(
        "/auth/change-password/",
        json={"old_password": PASSWORD, "new_password": NEW_PASSWORD},
    )
    assert res.status_code == 200, res.text[:200]
    rotated = [
        c for c in res.headers.get_list("set-cookie") if c.startswith(SESSION_COOKIE_NAME + "=")
    ]
    assert len(rotated) == 1, res.headers.get_list("set-cookie")
    new_key = rotated[0].split("=", 1)[1].split(";", 1)[0]
    assert new_key and new_key != old_key
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT COUNT(*) FROM sessions WHERE session_key = %s", (old_key,))
            assert cur.fetchone()[0] == 0, "re-login must delete the old row like cycle_key()"
            cur.execute("SELECT session_key FROM sessions WHERE user_id = %s", (user["id"],))
            keys = [row[0] for row in cur.fetchall()]
    assert keys == [new_key]
