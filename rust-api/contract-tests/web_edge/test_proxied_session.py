# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Session transparency on edge-proxied requests (PIDASHCONV-1304).

When the Rust edge proxies a request to Django, Django's own
``SessionMiddleware`` already handles the session cookie for the final
session state. The edge's ``SessionLayer`` must not append a second,
conflicting ``Set-Cookie`` computed from its pre-request snapshot:

- stale cookie + sign-in: the snapshot is empty with a cookie present,
  so the layer's delete rule would wipe the cookie Django just set and
  the sign-in silently fails;
- save-every-request + sign-out: the layer would re-save the stale
  snapshot over Django's flush, resurrecting the session.

Run::

    BASE_URL=http://127.0.0.1:18204 \
    DATABASE_URL="postgresql://rich@127.0.0.1:55304/pidash_1304" \
    pytest web_edge/test_proxied_session.py

No ``REDIS_URL`` is needed: the exercised endpoints (CSRF priming,
sign-in, ``/api/users/me/``) carry no throttle, so no counter reset.
"""

import uuid
from urllib.parse import urlsplit

import httpx
import pytest

from _harness.db import Database
from _harness.sessions import make_password_hash
from _harness.settings import base_url, database_url

PASSWORD = "Contract-Strong-Pass-1!x"
TAG = "we-"


def _unique_email() -> str:
    return f"{TAG}{uuid.uuid4().hex[:12]}@example.com"


def _unique_username() -> str:
    return f"{TAG}{uuid.uuid4().hex[:12]}"


def make_user(db: Database, email: str) -> dict:
    """Insert a users row (+ its preference row) for credential login."""
    uid = str(uuid.uuid4())
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO users (id, password, username, email, first_name, last_name,"
                " avatar, date_joined, created_at, updated_at, last_location,"
                " created_location, is_superuser, is_managed, is_password_expired,"
                " is_active, is_staff, is_email_verified, is_password_autoset, token,"
                " user_timezone, last_login_ip, last_logout_ip, last_login_medium,"
                " last_login_uagent, is_bot, display_name, is_email_valid,"
                " is_password_reset_required)"
                " VALUES (%s,%s,%s,%s,'','','', now(), now(), now(), '','',"
                " false,false,false,true,false,false,false,'', 'UTC',"
                " '','','email','',false,'',false,false)",
                (uid, make_password_hash(PASSWORD), _unique_username(), email),
            )
            cur.execute(
                "INSERT INTO user_notification_preferences (id, property_change, state_change,"
                " comment, mention, issue_completed, user_id, created_at, updated_at)"
                " VALUES (%s, true, true, true, true, true, %s, now(), now())",
                (str(uuid.uuid4()), uid),
            )
        conn.commit()
    return {"id": uid, "email": email, "password": PASSWORD}


def stale_session_key(db: Database) -> str:
    """A session-id cookie value with provably no session row."""
    key = f"{TAG}stale-{uuid.uuid4().hex}"
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT COUNT(*) FROM sessions WHERE session_key = %s", (key,))
            assert cur.fetchone()[0] == 0
    return key


def prime_csrf(client: httpx.Client) -> str:
    """CSRF-prime a jar-holding client; returns the token for the header."""
    res = client.get("/auth/get-csrf-token/")
    assert res.status_code == 200, f"CSRF priming failed: {res.status_code} {res.text[:200]!r}"
    return res.json()["csrf_token"]


def session_cookies(resp: httpx.Response) -> list:
    return [v for v in resp.headers.get_list("set-cookie") if v.startswith("session-id=")]


def plant_stale_cookie(client: httpx.Client, base: str, stale: str) -> None:
    # Plant the stale cookie for the backend's own host, like a browser
    # jar would hold it: a domain-less plant would survive alongside the
    # response's cookie instead of being replaced by it.
    client.cookies.set("session-id", stale, domain=urlsplit(base).hostname, path="/")


@pytest.fixture(scope="session")
def db() -> Database:
    return Database(database_url())


@pytest.fixture(scope="session")
def base() -> str:
    return base_url()


@pytest.fixture(autouse=True)
def clean(db):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM sessions WHERE user_id IN "
                "(SELECT id::text FROM users WHERE email LIKE 'we-%%')"
            )
            cur.execute(
                "DELETE FROM user_notification_preferences WHERE user_id IN "
                "(SELECT id FROM users WHERE email LIKE 'we-%%')"
            )
            cur.execute(
                "DELETE FROM profiles WHERE user_id IN "
                "(SELECT id FROM users WHERE email LIKE 'we-%%')"
            )
            cur.execute("DELETE FROM users WHERE email LIKE 'we-%%'")
            cur.execute("SELECT COUNT(*) FROM instances")
            if cur.fetchone()[0] == 0:
                db.make_instance()
        conn.commit()
    yield
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM sessions WHERE user_id IN "
                "(SELECT id::text FROM users WHERE email LIKE 'we-%%')"
            )
            cur.execute(
                "DELETE FROM user_notification_preferences WHERE user_id IN "
                "(SELECT id FROM users WHERE email LIKE 'we-%%')"
            )
            cur.execute(
                "DELETE FROM profiles WHERE user_id IN "
                "(SELECT id FROM users WHERE email LIKE 'we-%%')"
            )
            cur.execute("DELETE FROM users WHERE email LIKE 'we-%%'")
        conn.commit()


@pytest.fixture()
def user(db) -> dict:
    return make_user(db, _unique_email())


def test_stale_cookie_sign_in_ends_authenticated_with_single_cookie(base, db, user):
    # Case A: the browser holds a session-id whose row is gone. A valid
    # sign-in must end authenticated with exactly one session-id
    # Set-Cookie — Django's new cookie, not followed by the edge's
    # snapshot-driven delete.
    stale = stale_session_key(db)
    with httpx.Client(base_url=base, timeout=10, follow_redirects=False) as client:
        token = prime_csrf(client)
        plant_stale_cookie(client, base, stale)
        resp = client.post(
            "/auth/sign-in/",
            data={"email": user["email"], "password": user["password"]},
            headers={"X-CSRFToken": token},
        )
        found = session_cookies(resp)
        assert len(found) == 1, f"expected one session-id Set-Cookie, got {found!r}"
        assert "Max-Age=0" not in found[0], f"sign-in cookie is a delete: {found[0]!r}"
        # The jar now holds what a browser would hold; it must be authed.
        me = client.get("/api/users/me/")
        assert me.status_code == 200, f"expected authenticated, got {me.status_code}: {me.text[:200]!r}"


def test_proxied_unowned_method_emits_single_session_cookie(base, db):
    # Only POST is owned on /auth/sign-in/, so GET always proxies to
    # Django — flags on or off. Django answers 405 with its own single
    # session-id delete for the stale cookie; the edge must not append
    # a second one from its pre-request snapshot.
    stale = stale_session_key(db)
    with httpx.Client(base_url=base, timeout=10, follow_redirects=False) as client:
        plant_stale_cookie(client, base, stale)
        resp = client.get("/auth/sign-in/")
        assert resp.status_code == 405, f"expected 405, got {resp.status_code}: {resp.text[:200]!r}"
        found = session_cookies(resp)
        assert len(found) == 1, f"expected one session-id Set-Cookie, got {found!r}"
        assert "Max-Age=0" in found[0], f"expected a delete cookie, got {found[0]!r}"
