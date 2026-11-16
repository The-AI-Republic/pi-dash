# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""A session save that loses its row mid-request is Django's 400 (PIDASHCONV-1707).

The trigger is a common race: a user signs out in one tab while another
tab's request on a session-saving route is in flight. The sign-out
deletes the session row; the in-flight request's save then misses and
Django raises ``SessionInterrupted`` (a ``BadRequest``), answering 400
with its stock "Bad Request (400)" HTML page and no session cookie.

The race is made deterministic with a test-held row lock instead of
timing: the test locks the session row (``SELECT ... FOR UPDATE`` in an
open transaction), fires ``GET /auth/gitlab/`` — which writes
``request.session["host"]`` first on every branch, so the save always
runs — waits until the save's ``UPDATE`` is observed blocked in
``pg_locks``, then deletes the row and commits inside the holding
transaction. The ``UPDATE`` unblocks matching 0 rows on both backends.

Run::

    BASE_URL=http://127.0.0.1:18407 \
    DATABASE_URL="postgresql://rich@127.0.0.1:55304/pidash_1707" \
    pytest web_edge/test_session_save_race.py

against the Django oracle and the Rust server (with ``PIDASH_RUST_AUTH=1``,
so the initiate route is Rust-owned). No ``REDIS_URL`` is needed beyond
what the backend itself requires: the exercised endpoints (CSRF priming,
sign-in, gitlab initiate) carry no throttle.

Status, content type and cookie are pinned on both backends; the exact
stock-page bytes are pinned by the Rust unit test
(``middleware::session::tests::update_miss_answers_django_400_page``).
Under ``settings.test`` (``DEBUG=True``) the Django oracle renders the
technical traceback page instead of the stock bytes, which no port can
reproduce byte for byte (same precedent as
``auth_session/test_redirect_length.py``).
"""

import threading
import time
import uuid

import httpx
import psycopg
import pytest

from _harness.db import Database
from _harness.sessions import make_password_hash
from _harness.settings import base_url, database_url

PASSWORD = "Contract-Strong-Pass-1!x"
TAG = "sr-"

# The race request blocks on the test-held row lock; both timeouts must
# comfortably exceed the lock hold (seconds, not minutes).
RACE_HTTP_TIMEOUT = 30.0
BLOCKED_POLL_DEADLINE_SECS = 20.0


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


def _clean_rows(db: Database) -> None:
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM sessions WHERE user_id IN "
                f"(SELECT id::text FROM users WHERE email LIKE '{TAG}%%')"
            )
            cur.execute(
                "DELETE FROM user_notification_preferences WHERE user_id IN "
                f"(SELECT id FROM users WHERE email LIKE '{TAG}%%')"
            )
            cur.execute(
                "DELETE FROM profiles WHERE user_id IN "
                f"(SELECT id FROM users WHERE email LIKE '{TAG}%%')"
            )
            cur.execute(f"DELETE FROM users WHERE email LIKE '{TAG}%%'")
        conn.commit()


def prime_csrf(client: httpx.Client) -> str:
    """CSRF-prime a jar-holding client; returns the token for the header."""
    res = client.get("/auth/get-csrf-token/")
    assert res.status_code == 200, f"CSRF priming failed: {res.status_code} {res.text[:200]!r}"
    return res.json()["csrf_token"]


def session_cookies(resp: httpx.Response) -> list:
    return [v for v in resp.headers.get_list("set-cookie") if v.startswith("session-id=")]


def blocked_locks() -> list:
    """Ungranted locks across the scratch DB.

    A row-lock wait shows as ``locktype='transactionid'`` with a NULL
    relation, so every ungranted lock counts (nothing else contends on
    a scratch database).
    """
    with psycopg.connect(database_url(), autocommit=True) as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT pid, locktype, mode FROM pg_locks WHERE NOT granted")
            return cur.fetchall()


@pytest.fixture(scope="session")
def db() -> Database:
    return Database(database_url())


@pytest.fixture(scope="session")
def base() -> str:
    return base_url()


@pytest.fixture(autouse=True)
def clean(db):
    _clean_rows(db)
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT COUNT(*) FROM instances")
            if cur.fetchone()[0] == 0:
                db.make_instance()
        conn.commit()
    yield
    _clean_rows(db)


@pytest.fixture()
def user(db) -> dict:
    return make_user(db, _unique_email())


def test_save_losing_its_row_mid_request_answers_400(base, db, user):
    with httpx.Client(base_url=base, timeout=RACE_HTTP_TIMEOUT, follow_redirects=False) as client:
        token = prime_csrf(client)
        signed_in = client.post(
            "/auth/sign-in/",
            data={"email": user["email"], "password": user["password"]},
            headers={"X-CSRFToken": token},
        )
        assert signed_in.status_code == 302, (
            f"sign-in failed: {signed_in.status_code} {signed_in.text[:300]!r}"
        )
        key = client.cookies.get("session-id")
        assert key, f"no session-id after sign-in: {signed_in.headers.get_list('set-cookie')!r}"

        # Premise guard: the initiate route re-saves the session (302 +
        # a session Set-Cookie), so the race below exercises the save.
        warmup = client.get("/auth/gitlab/")
        assert warmup.status_code == 302, (
            f"warmup failed: {warmup.status_code} {warmup.text[:300]!r}"
        )
        assert session_cookies(warmup), "warmup set no session cookie: route does not save?"
        key = client.cookies.get("session-id")
        assert key, "lost session-id after warmup"

        # Hold the row; fire the race request; wait for its save UPDATE
        # to block. The holder uses autocommit OFF (an explicit txn).
        holder = psycopg.connect(database_url())
        try:
            hcur = holder.cursor()
            hcur.execute(
                "SELECT session_key FROM sessions WHERE session_key = %s FOR UPDATE",
                (key,),
            )
            assert hcur.fetchone(), "session row missing before race"
            assert blocked_locks() == [], blocked_locks()

            outcome: dict = {}

            def fire() -> None:
                with httpx.Client(
                    base_url=base,
                    timeout=RACE_HTTP_TIMEOUT,
                    follow_redirects=False,
                    cookies={"session-id": key},
                ) as race_client:
                    try:
                        outcome["resp"] = race_client.get("/auth/gitlab/")
                    except Exception as exc:  # noqa: BLE001 - surfaced below
                        outcome["exc"] = exc

            thread = threading.Thread(target=fire)
            thread.start()
            try:
                deadline = time.time() + BLOCKED_POLL_DEADLINE_SECS
                while time.time() < deadline:
                    if blocked_locks():
                        break
                    time.sleep(0.05)
                else:
                    raise AssertionError(
                        "save UPDATE never blocked: the request did not reach its save"
                    )
                # Mid-request delete, then release: the UPDATE unblocks
                # matching 0 rows.
                hcur.execute("DELETE FROM sessions WHERE session_key = %s", (key,))
                holder.commit()
            except BaseException:
                # Release the lock so a stuck request can finish, then
                # re-raise (a commit followed by rollback is a no-op).
                holder.rollback()
                thread.join(timeout=RACE_HTTP_TIMEOUT + 5)
                raise
            thread.join(timeout=RACE_HTTP_TIMEOUT + 5)
            assert not thread.is_alive(), "race request hung"
            if "exc" in outcome:
                raise outcome["exc"]
            resp = outcome["resp"]
        finally:
            holder.close()

    assert resp.status_code == 400, (resp.status_code, resp.content[:200])
    assert resp.headers.get("content-type") == "text/html; charset=utf-8", resp.headers.get(
        "content-type"
    )
    assert session_cookies(resp) == [], resp.headers.get_list("set-cookie")
    assert resp.content, "expected Django's HTML error page, got an empty body"
    assert b"<html" in resp.content, resp.content[:200]
