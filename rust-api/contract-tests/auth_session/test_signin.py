# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/sign-in/ + /auth/spaces/sign-in/ — form Views, always 302.

Success carries no ``error_code`` param and sets the ``session-id``
cookie; failure carries ``error_code``/``error_message``. The browser
flow is required: a token-less POST hits the custom CSRF failure page
(HTTP 200) instead of the View.
"""

from _harness.redirects import (
    assert_error_redirect,
    assert_success_redirect,
    location_of,
    query_of,
)
from _harness.sessions import SESSION_COOKIE_NAME


def test_success_redirects_without_error_code(browser, base, user):
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": user["password"]})
    location = assert_success_redirect(res, base=base)
    # The View passes get_redirection_path(user) ("onboarding", workspace
    # slug, ...) through get_safe_redirect_url, which drops any next_path
    # not starting with "/": success always lands on the bare base URL.
    assert location == base


def test_success_sets_session_cookie(browser, base, user):
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": user["password"]})
    assert res.status_code == 302
    set_cookies = res.headers.get_list("set-cookie")
    assert any(c.startswith(SESSION_COOKIE_NAME + "=") for c in set_cookies), set_cookies


def test_success_honors_next_path(browser, base, user):
    res = browser().post(
        "/auth/sign-in/",
        data={"email": user["email"], "password": user["password"], "next_path": "/dashboard"},
    )
    assert location_of(res) == f"{base}/?next_path=/dashboard"


def test_missing_password_redirects_with_error_code(browser, base, user):
    res = browser().post("/auth/sign-in/", data={"email": user["email"]})
    assert_error_redirect(res, base=base, error_code=5070)
    assert query_of(location_of(res))["error_message"] == "REQUIRED_EMAIL_PASSWORD_SIGN_IN"


def test_missing_email_redirects_with_error_code(browser, base):
    res = browser().post("/auth/sign-in/", data={"password": "whatever"})
    assert_error_redirect(res, base=base, error_code=5070)


def test_invalid_email_redirects_with_error_code(browser, base):
    res = browser().post("/auth/sign-in/", data={"email": "not-an-email", "password": "x"})
    assert_error_redirect(res, base=base, error_code=5075)


def test_unknown_user_redirects_with_error_code(browser, base):
    res = browser().post("/auth/sign-in/", data={"email": "as-nobody@example.com", "password": "x"})
    assert_error_redirect(res, base=base, error_code=5060)


def test_wrong_password_redirects_with_error_code(browser, base, user):
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": "Wrong-Pass-1!x"})
    assert_error_redirect(res, base=base, error_code=5065)


def test_wrong_password_sets_no_session_cookie(browser, user):
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": "Wrong-Pass-1!x"})
    assert res.status_code == 302
    assert not [c for c in res.headers.get_list("set-cookie") if c.startswith(SESSION_COOKIE_NAME + "=")]


def test_email_is_case_and_space_insensitive(browser, base, user):
    res = browser().post(
        "/auth/sign-in/",
        data={"email": f"  {user['email'].upper()}  ", "password": user["password"]},
    )
    assert_success_redirect(res, base=base)


def test_without_csrf_token_hits_failure_page(anon):
    res = anon.post("/auth/sign-in/", data={"email": "as-x@example.com", "password": "x"})
    assert res.status_code == 200
    assert "text/html" in res.headers["content-type"]
    assert "CSRF Verification Failed" in res.text
    assert "templates/csrf_failure.html" in res.text


def test_instance_not_configured(browser, base, db):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("DELETE FROM instances")
        conn.commit()
    res = browser().post("/auth/sign-in/", data={"email": "as-x@example.com", "password": "x"})
    assert_error_redirect(res, base=base, error_code=5000)


def test_space_success_redirects_to_space_base(browser, space_base, user):
    res = browser().post("/auth/spaces/sign-in/", data={"email": user["email"], "password": user["password"]})
    location = assert_success_redirect(res, base=space_base)
    assert location == space_base
    assert any(
        c.startswith(SESSION_COOKIE_NAME + "=") for c in res.headers.get_list("set-cookie")
    )


def test_space_failure_carries_error_code(browser, space_base, user):
    res = browser().post("/auth/spaces/sign-in/", data={"email": user["email"], "password": "Wrong-Pass-1!x"})
    assert_error_redirect(res, base=space_base, error_code=5065)


def test_space_failure_paths_use_space_base(browser, space_base):
    res = browser().post("/auth/spaces/sign-in/", data={"email": "bad", "password": "x"})
    assert_error_redirect(res, base=space_base, error_code=5075)
    res = browser().post("/auth/spaces/sign-in/", data={"email": "as-nobody@example.com", "password": "x"})
    assert_error_redirect(res, base=space_base, error_code=5060)
