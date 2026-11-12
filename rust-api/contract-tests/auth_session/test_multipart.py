# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""multipart/form-data posts on the email routes (PIDASHCONV-1349).

Django fills ``request.POST`` (the plain sign Views) and ``request.data``
(the DRF email-check, whose default parsers include ``MultiPartParser``)
from multipart bodies, so a multipart post behaves exactly like its
urlencoded twin. The Rust email module used to read only urlencoded
bodies: multipart sign-in/sign-up missed the CSRF field (failure page,
or a 5070/5040 REQUIRED redirect with a header token), email-check
answered 5010 EMAIL_REQUIRED, and space sign-out dropped ``next_path``.

``files=`` with ``(None, value)`` tuples renders plain field parts (no
filename); the primed browser client already carries the ``X-CSRFToken``
header. Every case asserts the exact status + body/location on whichever
backend ``BASE_URL`` points at.
"""

import psycopg

from _harness.redirects import (
    assert_error_redirect,
    assert_success_redirect,
    location_of,
    query_of,
)
from _harness.sessions import SESSION_COOKIE_NAME
from _harness.settings import database_url

from .conftest import _unique_email, reprime


def post_multipart(client, path, fields, *, csrf_header=True):
    """POST ``fields`` as multipart/form-data.

    ``fields`` may be a dict or a list of pairs (for duplicate names).
    """
    items = list(fields.items()) if isinstance(fields, dict) else list(fields)
    multipart = [(key, (None, value)) for key, value in items]
    headers = {"X-CSRFToken": client.cookies["csrftoken"]} if csrf_header else {}
    return client.post(path, files=multipart, headers=headers, follow_redirects=False)


def test_signin_multipart_success(browser, base, user):
    res = post_multipart(
        browser(), "/auth/sign-in/", {"email": user["email"], "password": user["password"]}
    )
    location = assert_success_redirect(res, base=base)
    assert location == base
    assert any(
        c.startswith(SESSION_COOKIE_NAME + "=") for c in res.headers.get_list("set-cookie")
    )


def test_signin_multipart_csrf_field_token(browser, base, user):
    # The csrfmiddlewaretoken POST field carries the secret (no header):
    # CsrfViewMiddleware reads it from request.POST on Django, and the
    # Rust gate must read it from the same multipart pairs.
    client = browser()
    del client.headers["X-CSRFToken"]
    res = post_multipart(
        client,
        "/auth/sign-in/",
        {
            "email": user["email"],
            "password": user["password"],
            "csrfmiddlewaretoken": client.cookies["csrftoken"],
        },
        csrf_header=False,
    )
    assert_success_redirect(res, base=base)


def test_signin_multipart_missing_fields_redirect_required(browser, base, user):
    res = post_multipart(browser(), "/auth/sign-in/", {"note": "no credentials here"})
    assert_error_redirect(res, base=base, error_code=5070)
    assert query_of(location_of(res))["error_message"] == "REQUIRED_EMAIL_PASSWORD_SIGN_IN"


def test_signin_multipart_honors_next_path(browser, base, user):
    res = post_multipart(
        browser(),
        "/auth/sign-in/",
        {"email": user["email"], "password": user["password"], "next_path": "/dashboard"},
    )
    assert location_of(res) == f"{base}/?next_path=/dashboard"


def test_signin_multipart_last_email_wins(browser, base, user):
    res = post_multipart(
        browser(),
        "/auth/sign-in/",
        [("email", "as-nobody@example.com"), ("email", user["email"]), ("password", user["password"])],
    )
    assert_success_redirect(res, base=base)


def test_space_signin_multipart_success(browser, space_base, user):
    res = post_multipart(
        browser(),
        "/auth/spaces/sign-in/",
        {"email": user["email"], "password": user["password"]},
    )
    location = assert_success_redirect(res, base=space_base)
    assert location == space_base


def test_signup_multipart_success(browser, base, db):
    email = _unique_email()
    res = post_multipart(
        browser(), "/auth/sign-up/", {"email": email, "password": "Fresh-Strong-Pass-9!x"}
    )
    assert location_of(res) == base
    assert any(
        c.startswith(SESSION_COOKIE_NAME + "=") for c in res.headers.get_list("set-cookie")
    )
    with psycopg.connect(database_url()) as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT id FROM users WHERE email = %s", (email,))
            assert cur.fetchone() is not None


def test_signup_multipart_missing_fields_redirect_required(browser, base):
    res = post_multipart(browser(), "/auth/sign-up/", {"email": _unique_email()})
    assert_error_redirect(res, base=base, error_code=5040)


def test_space_signup_multipart_success(browser, space_base):
    email = _unique_email()
    res = post_multipart(
        browser(),
        "/auth/spaces/sign-up/",
        {"email": email, "password": "Fresh-Strong-Pass-9!x"},
    )
    assert location_of(res) == space_base


def test_email_check_multipart_known_user(browser, user):
    res = post_multipart(browser(), "/auth/email-check/", {"email": user["email"]})
    assert res.status_code == 200, res.text[:200]
    assert res.json() == {"existing": True, "status": "CREDENTIAL"}


def test_email_check_multipart_unknown_email_is_magic_code(browser):
    res = post_multipart(browser(), "/auth/email-check/", {"email": _unique_email()})
    assert res.status_code == 200, res.text[:200]
    assert res.json() == {"existing": False, "status": "MAGIC_CODE"}


def test_email_check_multipart_missing_email(browser):
    res = post_multipart(browser(), "/auth/email-check/", {"note": "no email here"})
    assert res.status_code == 400
    assert res.json() == {"error_code": 5010, "error_message": "EMAIL_REQUIRED"}


def test_space_signout_multipart_honors_next_path(browser, space_base, user):
    client = browser()
    res = client.post(
        "/auth/spaces/sign-in/", data={"email": user["email"], "password": user["password"]}
    )
    assert res.status_code == 302
    reprime(client)
    res = post_multipart(client, "/auth/spaces/sign-out/", {"next_path": "/board"})
    assert location_of(res) == f"{space_base}/?next_path=/board"


def test_signout_multipart_app_base(browser, base, user):
    client = browser()
    res = client.post("/auth/sign-in/", data={"email": user["email"], "password": user["password"]})
    assert res.status_code == 302
    reprime(client)
    res = post_multipart(client, "/auth/sign-out/", {"next_path": "/board"})
    assert location_of(res) == base
