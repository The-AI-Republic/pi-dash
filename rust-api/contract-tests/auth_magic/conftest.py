# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stage-1 oracle: magic-link auth (PIDASHCONV-101).

Black-box pins of the six ``authentication/urls.py`` magic routes against a
live backend (Django today, Rust via the proxy tomorrow):

- ``POST /auth/magic-generate/`` and ``POST /auth/spaces/magic-generate/``
  (DRF ``APIView``; JSON ``{"email"}`` → 200 ``{"key"}``).
- ``POST /auth/magic-sign-in/`` and ``POST /auth/spaces/magic-sign-in/``
  (plain Django ``View``; form ``email``/``code``/``next_path`` → 302).
- ``POST /auth/magic-sign-up/`` and ``POST /auth/spaces/magic-sign-up/``
  (same shape; success creates the ``User`` row).

Run::

    cd rust-api/contract-tests
    BASE_URL=http://127.0.0.1:18001 \\
    DATABASE_URL=postgresql://postgres:ct101pass@127.0.0.1:15401/pidash \\
    CONTRACT_REDIS_URL=redis://127.0.0.1:16301/0 \\
        pytest auth_magic

Environment: ``BASE_URL``/``DATABASE_URL`` (harness ``env``) plus
``CONTRACT_REDIS_URL`` (host-reachable twin of the backend's ``REDIS_URL``;
the suite reads ``magic_<email>`` tokens and resets DRF throttle counters).
The backend needs ``EMAIL_HOST`` set (else every generate is
``SMTP_NOT_CONFIGURED``), ``ENABLE_MAGIC_LINK_LOGIN != "0"``, and
``WEB_URL``/``APP_BASE_URL``/``SPACE_BASE_URL`` configured (else every
sign-in/up redirect 500s); no worker is required — the ``magic_link`` Celery
task only queues, and the suite asserts on the Redis token, never on mail.

Plain Django ``View`` routes enforce CSRF over real HTTP (the Django test
client skips it), so every form POST here carries a fresh ``X-CSRFToken``
taken from the client's current ``csrftoken`` cookie.
"""

from __future__ import annotations

import uuid
from urllib.parse import parse_qs, urlsplit

import httpx
import pytest

from _harness import env
from _harness import redis as redis_helper
from _harness.seed import Seed, ensure_setup_done

SESSION_COOKIE = "session-id"


@pytest.fixture
def db():
    conn = env.connect()
    try:
        yield conn
    finally:
        conn.close()


@pytest.fixture
def seed(db):
    s = Seed(db)
    yield s
    tracked_users = [row_id for table, _, row_id in s._rows if table == "users"]
    with db.cursor() as cur:
        if tracked_users:
            # Server-created rows FK-block the user deletes below.
            cur.execute(
                "DELETE FROM profiles WHERE user_id = ANY(%s::uuid[])",
                (tracked_users,),
            )
            cur.execute(
                "DELETE FROM user_notification_preferences"
                " WHERE user_id = ANY(%s::uuid[])",
                (tracked_users,),
            )
    s.cleanup()


@pytest.fixture
def rdb():
    r = redis_helper.client()
    try:
        yield r
    finally:
        r.close()


@pytest.fixture(autouse=True)
def setup_done(db):
    ensure_setup_done(db)


@pytest.fixture(autouse=True)
def reset_throttle(rdb):
    redis_helper.clear_throttle_counters(rdb)


def fresh_email(prefix: str = "ct101") -> str:
    return f"{prefix}-{uuid.uuid4().hex[:8]}@example.com"


def csrf_client() -> httpx.Client:
    client = httpx.Client(base_url=env.base_url(), timeout=30.0)
    resp = client.get("/auth/get-csrf-token/")
    assert resp.status_code == 200, resp.text
    assert set(resp.json()) == {"csrf_token"}
    return client


def post_form(client: httpx.Client, path: str, data: dict) -> httpx.Response:
    """POST a form with the client's current CSRF secret as the header.

    The secret rotates on login; always re-read the jar instead of caching
    the token from ``/auth/get-csrf-token/``.
    """
    headers = {"X-CSRFToken": client.cookies["csrftoken"]}
    return client.post(path, data=data, headers=headers, follow_redirects=False)


def generate(client: httpx.Client, path: str, email: str) -> httpx.Response:
    return client.post(path, json={"email": email})


def location_query(resp: httpx.Response) -> dict:
    loc = resp.headers["location"]
    return {k: v[0] for k, v in parse_qs(urlsplit(loc).query).items()}


def location_path(resp: httpx.Response) -> str:
    return urlsplit(resp.headers["location"]).path


def session_rows(db, user_id: str) -> int:
    with db.cursor() as cur:
        cur.execute("SELECT count(*) FROM sessions WHERE user_id = %s", (user_id,))
        return cur.fetchone()[0]
