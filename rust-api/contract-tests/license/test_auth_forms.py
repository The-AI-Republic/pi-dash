# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Form-login Views (sign-in / sign-up / sign-out).

A token-less form POST hits the custom CSRF_FAILURE_VIEW: HTTP 200 with
the csrf_failure template. The redirect branches behind a valid token are
covered too, via a token primed from GET /auth/get-csrf-token/ (public on
both backends): every outcome is a 302 — errors to the admin base with
error_code/error_message query params, success to <base>/general/.
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


# ---------------------------------------------------------------------------
# Behind-token redirect branches (PIDASHCONV-827).
# ---------------------------------------------------------------------------

import re

import httpx
import pytest

from _harness import admin_client
from _harness.redirects import assert_error_redirect, assert_success_redirect, query_of
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


def _cookie_from(res, name):
    for raw in res.headers.get_list("set-cookie"):
        if raw.startswith(name + "="):
            return raw.split(";", 1)[0].split("=", 1)[1]
    raise AssertionError(f"no {name} cookie in {res.headers.get_list('set-cookie')!r}")


@pytest.fixture
def bare_world(db):
    """Instance with no admin yet: sign-up's happy path."""
    return {"db": db, "instance": db.make_instance()}


@pytest.fixture
def empty_db(db):
    """No instance at all (clean_db already wiped everything)."""
    return {"db": db}


def test_signup_success_creates_admin_and_logs_in(bare_world):
    db = bare_world["db"]
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-up/",
        data={
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
    _cookie_from(res, "csrftoken")  # login() rotates the CSRF secret
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT id, email, first_name, last_name, display_name,"
                " is_password_autoset, token, last_login_time, last_login_ip,"
                " last_login_uagent, last_login FROM users WHERE email = %s;",
                ("fresh@example.com",),
            )
            user = cur.fetchone()
            assert user is not None
            (uid, email, first, last, display, autoset, token, login_time, login_ip, uagent, last_login) = user
            assert (email, first, last, display) == ("fresh@example.com", "Fresh", "Signup", "fresh")
            assert autoset is False
            assert re.fullmatch(r"[0-9a-f]{64}", token)
            assert login_time is not None
            assert login_ip == "127.0.0.1"
            assert uagent.startswith("python-httpx")
            # login() fired user_logged_in -> update_last_login.
            assert last_login is not None
            cur.execute("SELECT company_name, background_color FROM profiles WHERE user_id = %s;", (uid,))
            profile = cur.fetchone()
            assert profile[0] == "Acme"
            assert re.fullmatch(r"#[0-9a-fA-F]{6}", profile[1])
            cur.execute("SELECT COUNT(*) FROM user_notification_preferences WHERE user_id = %s;", (uid,))
            assert cur.fetchone()[0] == 1
            cur.execute(
                "SELECT role, is_verified, created_by_id FROM instance_admins WHERE user_id = %s;",
                (uid,),
            )
            assert cur.fetchone() == (20, False, None)
            cur.execute(
                "SELECT is_setup_done, instance_name, is_telemetry_enabled FROM instances WHERE id = %s;",
                (bare_world["instance"]["id"],),
            )
            assert cur.fetchone() == (True, "Acme", False)
    with admin_client(session_key) as authed:
        body = authed.get("/api/instances/admins/session/").json()
        assert body["is_authenticated"] is True
        assert body["user"]["email"] == "fresh@example.com"


def test_signup_nulls_both_instance_audit_columns(bare_world):
    # The sign-up instance.save() runs while still anonymous
    # (admin.py:234, before user_login at :237), so Django NULLs both
    # audit columns on the pre-existing row (db/models/base.py:31-33).
    db = bare_world["db"]
    prior = db.make_user("priorauthor@example.com")
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "update instances set created_by_id = %s, updated_by_id = %s where id = %s;",
                (prior["id"], prior["id"], bare_world["instance"]["id"]),
            )
        conn.commit()
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-up/",
        data={
            "email": "auditnull@example.com",
            "password": STRONG_PASSWORD,
            "first_name": "Audit",
            "company_name": "Acme",
        },
    )
    assert_success_redirect(res, base=_admin_base())
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("select created_by_id, updated_by_id from instances;")
            assert cur.fetchone() == (None, None)


def test_signup_minimal_form_uses_defaults(bare_world):
    db = bare_world["db"]
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-up/",
        data={"email": "plain@example.com", "password": STRONG_PASSWORD, "first_name": "Plain"},
    )
    assert_success_redirect(res, base=_admin_base())
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT is_telemetry_enabled, instance_name FROM instances;")
            # Missing is_telemetry_enabled arrives as boolean True; missing
            # company_name arrives as "" and is stored verbatim.
            assert cur.fetchone() == (True, "")


def test_signup_missing_fields_echoes_payload(bare_world):
    from urllib.parse import parse_qs, urlparse

    client = _primed_client()
    res = client.post("/api/instances/admins/sign-up/", data={"email": "a@example.com"})
    location = assert_error_redirect(res, base=_admin_base(), error_code=5155)
    # keep_blank_values: the "" last_name/company_name echoes are in the URL.
    params = {k: v[0] for k, v in parse_qs(urlparse(location).query, keep_blank_values=True).items()}
    assert params == {
        "error_code": "5155",
        "error_message": "REQUIRED_ADMIN_EMAIL_PASSWORD_FIRST_NAME",
        "email": "a@example.com",
        "first_name": "False",
        "last_name": "",
        "company_name": "",
        "is_telemetry_enabled": "True",
    }


def test_signup_weak_password(bare_world):
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-up/",
        data={"email": "weak@example.com", "password": "password", "first_name": "Weak"},
    )
    location = assert_error_redirect(res, base=_admin_base(), error_code=5021)
    assert query_of(location)["email"] == "weak@example.com"


def test_signup_existing_user(bare_world):
    db = bare_world["db"]
    db.make_user("dup@example.com")
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-up/",
        data={"email": "Dup@Example.COM", "password": STRONG_PASSWORD, "first_name": "Dup"},
    )
    location = assert_error_redirect(res, base=_admin_base(), error_code=5180)
    assert query_of(location)["email"] == "dup@example.com"


def test_signup_invalid_email(bare_world):
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-up/",
        data={"email": "not-an-email", "password": STRONG_PASSWORD, "first_name": "Bad"},
    )
    assert_error_redirect(res, base=_admin_base(), error_code=5160)


def test_signup_second_admin_rejected(world):
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-up/",
        data={"email": "second@example.com", "password": STRONG_PASSWORD, "first_name": "Second"},
    )
    location = assert_error_redirect(res, base=_admin_base(), error_code=5150)
    assert query_of(location) == {"error_code": "5150", "error_message": "ADMIN_ALREADY_EXIST"}


def test_signup_no_instance(empty_db):
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-up/",
        data={"email": "x@example.com", "password": STRONG_PASSWORD, "first_name": "X"},
    )
    assert_error_redirect(res, base=_admin_base(), error_code=5000)


def test_signup_bad_telemetry_value_is_500_after_partial_commit(bare_world):
    # BooleanField.get_prep_value coerces case-sensitively: lowercase
    # 'true' raises on instance.save(), uncaught, AFTER the user, profile
    # and admin rows have committed. Both backends answer 500 here (Django
    # renders its DEBUG page, Rust the JSON generic branch), so this pins
    # the status and the partial commit, not the body.
    db = bare_world["db"]
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-up/",
        data={
            "email": "tele@example.com",
            "password": STRONG_PASSWORD,
            "first_name": "Tele",
            "is_telemetry_enabled": "true",
        },
    )
    assert res.status_code == 500
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT COUNT(*) FROM users WHERE email = %s;", ("tele@example.com",))
            assert cur.fetchone()[0] == 1
            cur.execute("SELECT COUNT(*) FROM instance_admins;")
            assert cur.fetchone()[0] == 1
            # The instance row itself was never written (name untouched).
            cur.execute("SELECT instance_name FROM instances;")
            assert cur.fetchone()[0] == "Contract Instance"
            # Write-order pin: user.save() committed BEFORE the admin insert
            # and the instance save that raised, so the user row is stamped
            # — but login() never ran, so the signal stamp stayed NULL.
            cur.execute(
                "SELECT last_login_time, last_login_ip, last_login_uagent, last_login"
                " FROM users WHERE email = %s;",
                ("tele@example.com",),
            )
            login_time, login_ip, uagent, last_login = cur.fetchone()
            assert login_time is not None
            assert login_ip == "127.0.0.1"
            assert uagent.startswith("python-httpx")
            assert last_login is None


def test_signup_missing_user_agent_is_500_with_no_admin_row(bare_world):
    # last_login_uagent is NOT NULL, so Django's user.save() raises before
    # the InstanceAdmin insert: the 500 leaves a user+profile but no admin
    # row — and a retry then reports the taken email (5180), not "admin
    # exists" (5150). Both backends answer 500 (bodies differ by design:
    # DEBUG page vs JSON), so this pins status + DB + the retry code.
    db = bare_world["db"]
    client = _primed_client()
    del client.headers["user-agent"]
    payload = {
        "email": "noua@example.com",
        "password": STRONG_PASSWORD,
        "first_name": "NoUa",
    }
    res = client.post("/api/instances/admins/sign-up/", data=payload)
    assert res.status_code == 500
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT COUNT(*) FROM users WHERE email = %s;", ("noua@example.com",))
            assert cur.fetchone()[0] == 1
            cur.execute(
                "SELECT last_login_time FROM users WHERE email = %s;", ("noua@example.com",)
            )
            assert cur.fetchone()[0] is None
            cur.execute("SELECT COUNT(*) FROM instance_admins;")
            assert cur.fetchone()[0] == 0
    retry = client.post("/api/instances/admins/sign-up/", data=payload)
    assert_error_redirect(retry, base=_admin_base(), error_code=5180)


def test_signin_success(world):
    db = world["db"]
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-in/",
        data={"email": "ADMIN@example.com", "password": world["admin"]["password"]},
    )
    location = assert_success_redirect(res, base=_admin_base())
    assert location == _admin_base() + "general/"
    session_key = _cookie_from(res, "admin-session-id")
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT last_login_time, last_login_ip, last_login FROM users WHERE email = %s;",
                ("admin@example.com",),
            )
            login_time, login_ip, last_login = cur.fetchone()
            assert login_time is not None
            assert login_ip == "127.0.0.1"
            # login() fired user_logged_in -> update_last_login.
            assert last_login is not None
    with admin_client(session_key) as authed:
        body = authed.get("/api/instances/admins/session/").json()
        assert body["is_authenticated"] is True
        assert body["user"]["email"] == "admin@example.com"


def test_signin_wrong_password(world):
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-in/",
        data={"email": "admin@example.com", "password": "WrongPass123!"},
    )
    location = assert_error_redirect(res, base=_admin_base(), error_code=5175)
    assert query_of(location)["email"] == "admin@example.com"


def test_signin_unknown_user(world):
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-in/",
        data={"email": "ghost@example.com", "password": "Whatever123!"},
    )
    assert_error_redirect(res, base=_admin_base(), error_code=5185)


def test_signin_inactive_user(world):
    world["db"].make_user("sleeper@example.com", is_active=False)
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-in/",
        data={"email": "sleeper@example.com", "password": "ContractPass123!"},
    )
    location = assert_error_redirect(res, base=_admin_base(), error_code=5190)
    # ADMIN_USER_DEACTIVATED carries no payload.
    assert query_of(location) == {"error_code": "5190", "error_message": "ADMIN_USER_DEACTIVATED"}


def test_signin_valid_non_admin_fails_auth(world):
    # A valid user without an InstanceAdmin row fails with
    # ADMIN_AUTHENTICATION_FAILED, not 403.
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-in/",
        data={"email": "member@example.com", "password": "ContractPass123!"},
    )
    assert_error_redirect(res, base=_admin_base(), error_code=5175)


def test_signin_missing_fields(world):
    client = _primed_client()
    res = client.post("/api/instances/admins/sign-in/", data={"email": "admin@example.com"})
    location = assert_error_redirect(res, base=_admin_base(), error_code=5170)
    assert query_of(location)["email"] == "admin@example.com"


def test_signin_invalid_email(world):
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-in/", data={"email": "not-an-email", "password": "x"}
    )
    assert_error_redirect(res, base=_admin_base(), error_code=5160)


def test_signin_no_instance(empty_db):
    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-in/",
        data={"email": "x@example.com", "password": "x"},
    )
    assert_error_redirect(res, base=_admin_base(), error_code=5000)


def test_signin_accepts_form_field_token(world):
    # The csrfmiddlewaretoken POST field (not the header) also passes the
    # gate: this client primes the cookie but sends no X-CSRFToken header.
    raw = httpx.Client(base_url=base_url(), timeout=15, follow_redirects=False)
    token = raw.get("/auth/get-csrf-token/").json()["csrf_token"]
    res = raw.post(
        "/api/instances/admins/sign-in/",
        data={
            "email": "admin@example.com",
            "password": "WrongPass123!",
            "csrfmiddlewaretoken": token,
        },
    )
    # 5175 proves the gate passed and the view ran.
    assert_error_redirect(res, base=_admin_base(), error_code=5175)


def test_signout_with_token_logs_out(world):
    db = world["db"]
    client = _primed_client(cookies={"admin-session-id": world["admin_key"]})
    res = client.post("/api/instances/admins/sign-out/", data={})
    assert res.status_code == 302
    assert res.headers["location"] == _admin_base().rstrip("/")
    assert res.headers["content-type"] == "text/html; charset=utf-8"
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT last_logout_time FROM users WHERE email = %s;", ("admin@example.com",)
            )
            assert cur.fetchone()[0] is not None
    # The flushed session no longer authenticates.
    with admin_client(world["admin_key"]) as stale:
        assert stale.get("/api/instances/admins/session/").json() == {"is_authenticated": False}


def test_signout_anonymous_with_token_still_redirects(anon_api):
    client = _primed_client()
    res = client.post("/api/instances/admins/sign-out/", data={})
    assert res.status_code == 302
    assert res.headers["location"] == _admin_base().rstrip("/")
    assert res.headers["content-type"] == "text/html; charset=utf-8"
