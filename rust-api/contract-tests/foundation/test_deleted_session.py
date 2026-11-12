"""Deleted-session arm (PIDASHCONV-723): a valid session whose
``_auth_user_id`` points at no ``users`` row must 401 byte-identical to
Django on every ported route — never the Rust 500.

Runs unchanged against Django and the Rust port (``BASE_URL`` /
``DATABASE_URL`` / ``SECRET_KEY``); both backends must satisfy the same
absolute assertions.
"""

from __future__ import annotations

import uuid

import pytest

from _harness.auth import authed_client, forge_session_key, get_secret_key
from _harness.seed import create_user

pytestmark = pytest.mark.contract

UNAUTHORIZED_BODY = b'{"detail":"Authentication credentials were not provided."}'

# One seedless, auth-required GET per sampled domain. The 401 fires before
# any domain read, so no workspace/project/world seeding is needed.
PROBE_ROUTES = [
    "/api/users/me/activities/",  # app_workspace (actor_timezone 500 pre-fix)
    "/api/workspaces/",  # app_workspace (actor-first list)
    "/api/users/me/auto-pm/",  # loop (license resolve_actor path)
]

PASSWORD_FIELD = "pbkdf2_sha256$10000$deleted-session-seed$ZmFrZXNhbHQ="


def _unique_tag() -> str:
    return uuid.uuid4().hex[:10]


def _delete_session(db_conn, session_key: str) -> None:
    with db_conn.cursor() as cur:
        cur.execute(
            "DELETE FROM sessions WHERE session_key = %s", (session_key,)
        )


def _delete_user(db_conn, user_id: str) -> None:
    with db_conn.cursor() as cur:
        cur.execute("DELETE FROM users WHERE id = %s", (user_id,))


def _forge_missing_user_session(db_conn, secret: str) -> str:
    """A live session row for a user that never existed (random UUID)."""
    return forge_session_key(
        db_conn,
        user_id=str(uuid.uuid4()),
        password_field=PASSWORD_FIELD,
        secret=secret,
    )


@pytest.mark.parametrize("route", PROBE_ROUTES)
def test_missing_user_session_401s_like_django(base_url, db_conn, route):
    """Never-existed user id: 401 with Django's exact body on each route."""
    secret = get_secret_key()
    session_key = _forge_missing_user_session(db_conn, secret)
    try:
        client = authed_client(base_url, session_key)
        try:
            resp = client.get(route)
        finally:
            client.close()
    finally:
        _delete_session(db_conn, session_key)
    assert resp.status_code == 401, (route, resp.status_code, resp.content[:200])
    assert resp.content == UNAUTHORIZED_BODY, (route, resp.content[:200])


def test_deleted_user_session_401s(base_url, db_conn):
    """True stale session: user created, session forged, user row deleted."""
    secret = get_secret_key()
    tag = _unique_tag()
    user = create_user(
        db_conn,
        email=f"deleted-{tag}@example.com",
        username=f"deleted-{tag}",
        password_field=PASSWORD_FIELD,
    )
    session_key = forge_session_key(
        db_conn,
        user_id=user["id"],
        password_field=PASSWORD_FIELD,
        secret=secret,
    )
    try:
        _delete_user(db_conn, user["id"])
        client = authed_client(base_url, session_key)
        try:
            resp = client.get("/api/users/me/activities/")
        finally:
            client.close()
    finally:
        _delete_session(db_conn, session_key)
        _delete_user(db_conn, user["id"])
    assert resp.status_code == 401, (resp.status_code, resp.content[:200])
    assert resp.content == UNAUTHORIZED_BODY, resp.content[:200]


def test_live_user_session_still_passes(base_url, db_conn):
    """Control: the same forge for an existing user must stay authenticated.

    Guards a vacuous pass — a broken forge (wrong SECRET_KEY) would 401
    everywhere and make the tests above pass for the wrong reason.
    """
    secret = get_secret_key()
    tag = _unique_tag()
    user = create_user(
        db_conn,
        email=f"live-{tag}@example.com",
        username=f"live-{tag}",
        password_field=PASSWORD_FIELD,
    )
    session_key = forge_session_key(
        db_conn,
        user_id=user["id"],
        password_field=PASSWORD_FIELD,
        secret=secret,
    )
    try:
        client = authed_client(base_url, session_key)
        try:
            resp = client.get("/api/users/me/activities/")
        finally:
            client.close()
    finally:
        _delete_session(db_conn, session_key)
        _delete_user(db_conn, user["id"])
    assert resp.status_code == 200, (resp.status_code, resp.content[:300])
