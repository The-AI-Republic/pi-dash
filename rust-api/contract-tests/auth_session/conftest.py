# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Fixtures: seed the auth world straight into Postgres, drive HTTP only.

World (every row prefixed ``as-`` so teardown only touches our rows):

- one ``instances`` row with ``is_setup_done=True`` (recreated after every
  wipe; tests that need it gone delete it explicitly)
- users with Django-compatible PBKDF2 password fields (stdlib only):
  ``user`` (credential login), ``autoset_user`` (``is_password_autoset``)

Run::

    BASE_URL=http://127.0.0.1:8140 \\
    DATABASE_URL="postgresql://irichard@/pidash_contract_100?host=/tmp" \\
    SECRET_KEY=<server SECRET_KEY> REDIS_URL=redis://127.0.0.1:6379/11 \\
    pytest auth_session

``REDIS_URL`` is required: the email-check/forgot-password Views carry
``AuthenticationThrottle`` (30/min per IP in the redis-backed default
cache) and every test starts from flushed counters. ``SECRET_KEY`` must
equal the live server's key: authenticated cases mint ``session-id``
cookies with :mod:`_harness.sessions` (stdlib replica of the custom
session engine). The suite never imports Django and never uses its test
client; form POSTs go through the browser flow (GET ``get-csrf-token/``
first, send ``X-CSRFToken``) because the plain Views enforce CSRF.
"""

import os
import uuid

import httpx
import psycopg
import pytest

from _harness.db import Database
from _harness.sessions import SESSION_COOKIE_NAME, login as mint_session
from _harness.sessions import make_password_hash
from _harness.settings import base_url, database_url, secret_key
from _harness.throttle import reset_throttle_counters

PASSWORD = "Contract-Strong-Pass-1!x"
NEW_PASSWORD = "Contract-Strong-Pass-2!y"
WEAK_PASSWORD = "123"

TAG = "as-"


def _unique_email() -> str:
    return f"{TAG}{uuid.uuid4().hex[:12]}@example.com"


def _unique_username() -> str:
    return f"{TAG}{uuid.uuid4().hex[:12]}"


def make_user(db: Database, email: str, password: str = PASSWORD, *, autoset: bool = False) -> dict:
    """Insert a users row (+ its signal-created preference row)."""
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
                " false,false,false,true,false,false,%s,'', 'UTC',"
                " '','','email','',false,'',false,false)",
                (uid, make_password_hash(password), _unique_username(), email, autoset),
            )
            cur.execute(
                "INSERT INTO user_notification_preferences (id, property_change, state_change,"
                " comment, mention, issue_completed, user_id, created_at, updated_at)"
                " VALUES (%s, true, true, true, true, true, %s, now(), now())",
                (str(uuid.uuid4()), uid),
            )
        conn.commit()
    return {"id": uid, "email": email, "password": password}


def login_cookies(user: dict) -> dict:
    """Mint a ``session-id`` cookie for a seeded user (stdlib, no Django)."""
    secret_key()
    with psycopg.connect(database_url()) as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT password FROM users WHERE id = %s", (user["id"],))
            password_field = cur.fetchone()[0]
        cookies = mint_session(conn, user["id"], password_field)
    return cookies


def primed_client(base: str, cookies: dict | None = None) -> httpx.Client:
    """Browser-like client: CSRF cookie jar + ``X-CSRFToken`` header set."""
    client = httpx.Client(base_url=base, timeout=10, follow_redirects=False)
    if cookies:
        client.cookies.update(cookies)
    reprime(client)
    return client


def reprime(client: httpx.Client) -> str:
    """Refresh the ``X-CSRFToken`` header from a fresh token GET.

    ``django.contrib.auth.login`` rotates the CSRF secret, so any POST
    after a sign-in on the same client must re-prime first — like a
    browser re-reading its cookie. Returns the new token.
    """
    res = client.get("/auth/get-csrf-token/")
    assert res.status_code == 200, f"CSRF priming failed: {res.status_code} {res.text[:200]!r}"
    token = res.json()["csrf_token"]
    client.headers["X-CSRFToken"] = token
    return token


@pytest.fixture(scope="session")
def db() -> Database:
    return Database(database_url())


@pytest.fixture(scope="session")
def base() -> str:
    return base_url()


@pytest.fixture(scope="session")
def _env():
    secret_key()
    if not os.environ.get("REDIS_URL"):
        raise RuntimeError("contract suite requires env REDIS_URL (throttle-counter resets)")
    return True


@pytest.fixture(autouse=True)
def clean(db, _env):
    # Sweep rows with FKs into users that Database.reset() does not know
    # about (signal-created preference rows, login-created profiles) so
    # the reset below never hits a foreign key — this also clears rows
    # left by ad-hoc probes against the same database.
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("DELETE FROM user_notification_preferences")
            cur.execute("DELETE FROM profiles")
        conn.commit()
    db.reset()
    reset_throttle_counters()
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT COUNT(*) FROM instances")
            if cur.fetchone()[0] == 0:
                db.make_instance()
    yield
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM sessions WHERE user_id IN "
                "(SELECT id::text FROM users WHERE email LIKE 'as-%%')"
            )
            cur.execute(
                "DELETE FROM user_notification_preferences WHERE user_id IN "
                "(SELECT id FROM users WHERE email LIKE 'as-%%')"
            )
            cur.execute(
                "DELETE FROM profiles WHERE user_id IN "
                "(SELECT id FROM users WHERE email LIKE 'as-%%')"
            )
            cur.execute("DELETE FROM users WHERE email LIKE 'as-%%'")
        conn.commit()


@pytest.fixture()
def user(db) -> dict:
    return make_user(db, _unique_email())


@pytest.fixture()
def autoset_user(db) -> dict:
    return make_user(db, _unique_email(), autoset=True)


@pytest.fixture()
def anon(base) -> httpx.Client:
    with httpx.Client(base_url=base, timeout=10, follow_redirects=False) as client:
        yield client


@pytest.fixture()
def browser(base):
    clients: list = []

    def _make(cookies: dict | None = None) -> httpx.Client:
        client = primed_client(base, cookies)
        clients.append(client)
        return client

    yield _make
    for client in clients:
        client.close()


@pytest.fixture()
def session_cookies(user) -> dict:
    return login_cookies(user)


@pytest.fixture()
def space_base(base) -> str:
    return base + "/spaces"
