# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/sign-up/ + /auth/spaces/sign-up/ — form Views, always 302."""

import psycopg

from _harness.redirects import assert_error_redirect, assert_success_redirect, location_of
from _harness.sessions import SESSION_COOKIE_NAME
from _harness.settings import database_url

from .conftest import _unique_email


def _assert_prefs_row(email):
    """`create_user_notification` post_save receiver: one prefs row, all flags true."""
    with psycopg.connect(database_url()) as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT id FROM users WHERE email = %s", (email,))
            row = cur.fetchone()
            assert row is not None
            cur.execute(
                "SELECT property_change, state_change, comment, mention, issue_completed,"
                " workspace_id, project_id FROM user_notification_preferences"
                " WHERE user_id = %s",
                (row[0],),
            )
            prefs = cur.fetchall()
            assert len(prefs) == 1
            assert prefs[0] == (True, True, True, True, True, None, None)


def test_success_creates_user_and_signs_in(browser, base, db):
    email = _unique_email()
    res = browser().post("/auth/sign-up/", data={"email": email, "password": "Fresh-Strong-Pass-9!x"})
    assert location_of(res) == base
    assert any(
        c.startswith(SESSION_COOKIE_NAME + "=") for c in res.headers.get_list("set-cookie")
    )
    _assert_prefs_row(email)


def test_existing_user_redirects_with_error_code(browser, base, user):
    res = browser().post("/auth/sign-up/", data={"email": user["email"], "password": user["password"]})
    assert_error_redirect(res, base=base, error_code=5030)


def test_missing_fields_redirect_with_error_code(browser, base):
    res = browser().post("/auth/sign-up/", data={"email": _unique_email()})
    assert_error_redirect(res, base=base, error_code=5040)


def test_invalid_email_redirects_with_error_code(browser, base):
    res = browser().post("/auth/sign-up/", data={"email": "not-an-email", "password": "x"})
    assert_error_redirect(res, base=base, error_code=5045)


def test_space_success_redirects_to_space_base(browser, space_base, db):
    email = _unique_email()
    res = browser().post("/auth/spaces/sign-up/", data={"email": email, "password": "Fresh-Strong-Pass-9!x"})
    assert location_of(res) == space_base
    assert any(
        c.startswith(SESSION_COOKIE_NAME + "=") for c in res.headers.get_list("set-cookie")
    )
    _assert_prefs_row(email)


def test_space_existing_user_redirects_with_error_code(browser, space_base, user):
    res = browser().post(
        "/auth/spaces/sign-up/", data={"email": user["email"], "password": user["password"]}
    )
    assert_error_redirect(res, base=space_base, error_code=5030)
