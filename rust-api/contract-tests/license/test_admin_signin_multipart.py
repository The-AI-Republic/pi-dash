# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""multipart/form-data posts on the admin form views (PIDASHCONV-1367).

Django fills ``request.POST`` from multipart bodies (``HttpRequest.
_load_post_and_files``), so the sign-in/sign-up Views read their fields
— and ``CsrfViewMiddleware`` reads ``csrfmiddlewaretoken`` — from a
multipart post exactly like from its urlencoded twin. The Rust admin
module used to read only urlencoded bodies: a multipart sign-in/sign-up
missed the CSRF field (failure page, or a REQUIRED_* error redirect
with a header token) and created no session.

``files=`` with ``(None, value)`` tuples renders plain field parts (no
filename). Every case asserts the exact status + location on whichever
backend ``BASE_URL`` points at.
"""

import httpx
import pytest

from _harness import admin_client
from _harness.redirects import assert_success_redirect
from _harness.settings import base_url

STRONG_PASSWORD = "Fresh-Strong-Pass-9!x"  # zxcvbn >= 3 on both estimators


def _admin_base():
    return base_url() + "/god-mode/"


def _primed_client(cookies=None):
    """Browser-like client: CSRF cookie jar + X-CSRFToken header set."""
    client = httpx.Client(base_url=base_url(), timeout=15, follow_redirects=False)
    if cookies:
        client.cookies.update(cookies)
    res = client.get("/auth/get-csrf-token/")
    assert res.status_code == 200, f"CSRF priming failed: {res.status_code}"
    client.headers["X-CSRFToken"] = res.json()["csrf_token"]
    return client


def _raw_client_with_cookie():
    """Client with a primed CSRF cookie but no X-CSRFToken header."""
    client = httpx.Client(base_url=base_url(), timeout=15, follow_redirects=False)
    token = client.get("/auth/get-csrf-token/").json()["csrf_token"]
    return client, token


def _post_multipart(client, path, fields):
    multipart = [(key, (None, value)) for key, value in fields.items()]
    return client.post(path, files=multipart, follow_redirects=False)


def _cookie_from(res, name):
    for raw in res.headers.get_list("set-cookie"):
        if raw.startswith(name + "="):
            return raw.split(";", 1)[0].split("=", 1)[1]
    raise AssertionError(f"no {name} cookie in {res.headers.get_list('set-cookie')!r}")


@pytest.fixture
def bare_world(db):
    """Instance with no admin yet: sign-up's happy path."""
    return {"db": db, "instance": db.make_instance()}


def test_signin_multipart_success(world):
    client = _primed_client()
    res = _post_multipart(
        client,
        "/api/instances/admins/sign-in/",
        {"email": "ADMIN@example.com", "password": world["admin"]["password"]},
    )
    location = assert_success_redirect(res, base=_admin_base())
    assert location == _admin_base() + "general/"
    session_key = _cookie_from(res, "admin-session-id")
    with admin_client(session_key) as authed:
        body = authed.get("/api/instances/admins/session/").json()
        assert body["is_authenticated"] is True
        assert body["user"]["email"] == "admin@example.com"


def test_signin_multipart_csrf_field_token(world):
    # The csrfmiddlewaretoken POST field carries the secret (no header):
    # CsrfViewMiddleware reads it from request.POST on Django, and the
    # Rust gate must read it from the same multipart pairs.
    client, token = _raw_client_with_cookie()
    res = _post_multipart(
        client,
        "/api/instances/admins/sign-in/",
        {
            "email": "admin@example.com",
            "password": world["admin"]["password"],
            "csrfmiddlewaretoken": token,
        },
    )
    location = assert_success_redirect(res, base=_admin_base())
    assert location == _admin_base() + "general/"
    _cookie_from(res, "admin-session-id")


def test_signup_multipart_success_creates_admin_and_logs_in(bare_world):
    db = bare_world["db"]
    client = _primed_client()
    res = _post_multipart(
        client,
        "/api/instances/admins/sign-up/",
        {
            "email": "Fresh@Example.COM",
            "password": STRONG_PASSWORD,
            "first_name": "Fresh",
            "last_name": "Signup",
            "company_name": "Acme",
            "is_telemetry_enabled": "False",
        },
    )
    location = assert_success_redirect(res, base=_admin_base())
    assert location == _admin_base() + "general/"
    session_key = _cookie_from(res, "admin-session-id")
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT id FROM users WHERE email = %s;", ("fresh@example.com",)
            )
            row = cur.fetchone()
            assert row is not None
            cur.execute("SELECT COUNT(*) FROM instance_admins;")
            assert cur.fetchone()[0] == 1
    with admin_client(session_key) as authed:
        body = authed.get("/api/instances/admins/session/").json()
        assert body["is_authenticated"] is True
        assert body["user"]["email"] == "fresh@example.com"


def test_signup_multipart_csrf_field_token(bare_world):
    client, token = _raw_client_with_cookie()
    res = _post_multipart(
        client,
        "/api/instances/admins/sign-up/",
        {
            "email": "fieldtoken@example.com",
            "password": STRONG_PASSWORD,
            "first_name": "Field",
            "csrfmiddlewaretoken": token,
        },
    )
    location = assert_success_redirect(res, base=_admin_base())
    assert location == _admin_base() + "general/"
    _cookie_from(res, "admin-session-id")


def test_signout_multipart_form_token_logs_out(world):
    db = world["db"]
    client = _primed_client(cookies={"admin-session-id": world["admin_key"]})
    del client.headers["X-CSRFToken"]
    token = client.get("/auth/get-csrf-token/").json()["csrf_token"]
    res = _post_multipart(
        client, "/api/instances/admins/sign-out/", {"csrfmiddlewaretoken": token}
    )
    assert res.status_code == 302
    assert res.headers["location"] == _admin_base().rstrip("/")
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT last_logout_time FROM users WHERE email = %s;",
                ("admin@example.com",),
            )
            assert cur.fetchone()[0] is not None
    with admin_client(world["admin_key"]) as stale:
        assert stale.get("/api/instances/admins/session/").json() == {
            "is_authenticated": False
        }
