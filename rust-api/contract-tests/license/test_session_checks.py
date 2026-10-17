# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stale/wrong-backend/inactive admin sessions (PIDASHCONV-1097).

Django verifies three session facts the license instance+admin handlers
must also verify: ``_auth_user_backend`` is the model backend,
``_auth_user_hash`` matches the row's password field (else the session is
flushed to anonymous), and the user is active (DRF ``SessionAuthentication``
returns ``None`` for inactive users). Three broken-session shapes:

- ``stale``: minted, then the user's password is rotated (the hash in the
  session no longer matches the row) — the old cookie is replayed.
- ``backend``: hand-crafted session row with a wrong ``_auth_user_backend``.
- ``inactive``: minted, then the user is deactivated.

Expected (Django) behavior pinned here: 401 with the DRF body on every
guarded DRF route, the bare ``{"is_authenticated": False}`` flag on the
AllowAny session endpoint, UTC datetimes on the AllowAny GET, NULL
``updated_by`` stamps on signup-visited, and no logout stamps on sign-out
for any broken shape — *including* inactive-with-valid-hash on sign-out:
``ModelBackend.get_user`` returns ``None`` for inactive users
(``user_can_authenticate``), so ``get_user`` yields anonymous even in
plain Views, and the sign-out body raises out of
``User.objects.get(pk=None)`` before stamping. Verified against live
Django (see ``test_broken_session_signout_writes_no_stamps``).

Runs unchanged against Django and the Rust port (``BASE_URL`` /
``DATABASE_URL`` / ``SECRET_KEY``); both backends must satisfy the same
absolute assertions.
"""

from __future__ import annotations

import json
from datetime import datetime, timedelta, timezone

import httpx
import pytest

from _harness import signing
from _harness.http import ContractClient
from _harness.settings import base_url, secret_key

UNAUTH = {"detail": "Authentication credentials were not provided."}
BARE_SESSION = {"is_authenticated": False}
WRONG_BACKEND = "django.contrib.auth.backends.RemoteUserBackend"

CONDITIONS = ["stale", "backend", "inactive"]


def _mint_raw(db, user_id, password_hash, secret, backend=signing.AUTH_BACKEND, auth_hash=None):
    """Insert one admin-cookie session row with an explicit payload."""
    payload = {
        "_auth_user_id": str(user_id),
        "_auth_user_backend": backend,
        "_auth_user_hash": (
            auth_hash
            if auth_hash is not None
            else signing.salted_hmac(signing.AUTH_HASH_SALT, password_hash, secret).hex()
        ),
        "device_info": {
            "user_agent": "contract-suite",
            "ip_address": "127.0.0.1",
            "domain": "contract",
        },
    }
    data = signing.encode_session(payload, secret)
    key = signing.new_session_key()
    expires = datetime.now(timezone.utc) + timedelta(hours=1)
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO sessions (session_key, session_data, expire_date, user_id, device_info)"
                " VALUES (%s, %s, %s, %s, %s::jsonb)",
                (key, data, expires, str(user_id), json.dumps(payload["device_info"])),
            )
        conn.commit()
    return key


def _rotate_password(db, user_id):
    new_hash = signing.make_password_hash("RotatedPass999!")
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("UPDATE users SET password = %s WHERE id = %s;", (new_hash, str(user_id)))
        conn.commit()
    return new_hash


def _deactivate(db, user_id):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("UPDATE users SET is_active = false WHERE id = %s;", (str(user_id),))
        conn.commit()


def _set_timezone(db, user_id, zone):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("UPDATE users SET user_timezone = %s WHERE id = %s;", (zone, str(user_id)))
        conn.commit()


def _broken_key(world, condition):
    """An admin-cookie session key in the named broken state."""
    db = world["db"]
    secret = secret_key()
    admin = world["admin"]
    if condition == "stale":
        key = _mint_raw(db, admin["id"], admin["password_hash"], secret)
        _rotate_password(db, admin["id"])
        return key
    if condition == "backend":
        return _mint_raw(db, admin["id"], admin["password_hash"], secret, backend=WRONG_BACKEND)
    if condition == "inactive":
        key = _mint_raw(db, admin["id"], admin["password_hash"], secret)
        _deactivate(db, admin["id"])
        return key
    raise AssertionError(f"unknown condition {condition!r}")


def _session_client(key):
    """Session-cookie client that rides out the stock anon throttle.

    Broken sessions authenticate as nobody, so every request here counts
    against DRF's 30/min anon throttle — over a full-suite run the window
    fills and Django answers 429. A 429 is the server asking us to wait,
    never a contract answer, so retry it (harness ``ContractClient``)
    instead of failing the run.
    """
    return ContractClient(
        base_url=base_url(),
        cookies={signing.ADMIN_SESSION_COOKIE: key},
        timeout=15,
    )


def _primed_client_with_cookie(cookie_name, cookie_value):
    """Browser-like client: CSRF cookie jar + header, plus one session cookie."""
    client = ContractClient(base_url=base_url(), timeout=15, follow_redirects=False)
    client.cookies.set(cookie_name, cookie_value)
    res = client.get("/auth/get-csrf-token/")
    assert res.status_code == 200, f"CSRF priming failed: {res.status_code}"
    client.headers["X-CSRFToken"] = res.json()["csrf_token"]
    return client


def _instance_row(db):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT instance_name, updated_by_id FROM instances;")
            return cur.fetchone()


def _logout_stamps(db, email):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT last_logout_ip, last_logout_time FROM users WHERE email = %s;", (email,)
            )
            return cur.fetchone()


@pytest.mark.parametrize("condition", CONDITIONS)
def test_broken_session_denied_on_guarded_routes(world, condition):
    """Stale-hash, wrong-backend and inactive cookies all 401 on the five
    guarded DRF routes (PATCH + admins CRUD + me), with Django's exact
    body — and write nothing."""
    key = _broken_key(world, condition)
    doomed = world["db"].make_user("doomed@example.com")
    row = world["db"].make_admin(world["instance"]["id"], doomed["id"])
    with _session_client(key) as client:
        res = client.patch("/api/instances/", json={"instance_name": "Nope"})
        assert res.status_code == 401, (condition, res.status_code, res.text[:200])
        assert res.json() == UNAUTH
        res = client.get("/api/instances/admins/")
        assert res.status_code == 401, (condition, res.status_code, res.text[:200])
        assert res.json() == UNAUTH
        res = client.post("/api/instances/admins/", json={"email": "member@example.com"})
        assert res.status_code == 401, (condition, res.status_code, res.text[:200])
        assert res.json() == UNAUTH
        res = client.delete(f"/api/instances/admins/{row['id']}/")
        assert res.status_code == 401, (condition, res.status_code, res.text[:200])
        assert res.json() == UNAUTH
        res = client.get("/api/instances/admins/me/")
        assert res.status_code == 401, (condition, res.status_code, res.text[:200])
        assert res.json() == UNAUTH
    # The 401 fires before any handler body: nothing was written.
    name, updated_by = _instance_row(world["db"])
    assert (name, updated_by) == ("Contract Instance", None)
    with world["db"].connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT COUNT(*) FROM instance_admins WHERE deleted_at IS NULL;")
            assert cur.fetchone()[0] == 2
            cur.execute("SELECT deleted_at FROM instance_admins WHERE id = %s;", (row["id"],))
            assert cur.fetchone()[0] is None


@pytest.mark.parametrize("condition", CONDITIONS)
def test_broken_session_reports_bare_flag(world, condition):
    key = _broken_key(world, condition)
    with _session_client(key) as client:
        res = client.get("/api/instances/admins/session/")
    assert res.status_code == 200
    assert res.json() == BARE_SESSION


@pytest.mark.parametrize("condition", CONDITIONS)
def test_broken_session_get_renders_utc(world, condition):
    """The AllowAny GET renders datetimes in the request zone — anonymous
    (which every broken shape is) means UTC, whatever the row stores."""
    _set_timezone(world["db"], world["admin"]["id"], "America/New_York")
    key = _broken_key(world, condition)
    with _session_client(key) as client:
        body = client.get("/api/instances/").json()
    assert body["instance"]["created_at"].endswith("Z"), body["instance"]["created_at"]
    assert body["instance"]["updated_at"].endswith("Z"), body["instance"]["updated_at"]


def test_fresh_session_get_renders_user_zone(world):
    """Control: the same GET with a valid cookie renders the stored zone,
    so the UTC assertions above are not vacuous."""
    _set_timezone(world["db"], world["admin"]["id"], "America/New_York")
    with _session_client(world["admin_key"]) as client:
        body = client.get("/api/instances/").json()
    assert not body["instance"]["created_at"].endswith("Z"), body["instance"]["created_at"]


@pytest.mark.parametrize("condition", CONDITIONS)
def test_broken_session_signup_visited_stamps_null(world, condition):
    """The AllowAny signup-visited POST still 204s, but stamps no author:
    anonymous posts leave ``updated_by`` NULL."""
    key = _broken_key(world, condition)
    with _session_client(key) as client:
        res = client.post("/api/instances/admins/sign-up-screen-visited/")
    assert res.status_code == 204
    with world["db"].connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT is_signup_screen_visited, updated_by_id FROM instances;")
            visited, updated_by = cur.fetchone()
    assert visited is True
    assert updated_by is None


def test_fresh_session_signup_visited_stamps_author(world):
    """Control: a valid cookie stamps the acting admin."""
    with _session_client(world["admin_key"]) as client:
        res = client.post("/api/instances/admins/sign-up-screen-visited/")
    assert res.status_code == 204
    _, updated_by = _instance_row(world["db"])
    assert str(updated_by) == world["admin"]["id"]


@pytest.mark.parametrize("condition", CONDITIONS)
def test_broken_session_signout_writes_no_stamps(world, condition):
    """Sign-out is a plain View: it always 302s, but a session that fails
    the backend/hash/active check authenticates to nobody, so no logout
    columns are stamped. The inactive case corrects the issue text's
    suggested fix (which claimed no ``is_active`` gate on sign-out):
    ``ModelBackend.get_user`` gates inactive users before the hash check,
    so ``request.user`` is anonymous and the ``get(pk=None)`` raises
    before any stamp — probed live: inactive sign-out leaves
    ``last_logout_ip=''`` and ``last_logout_time=NULL`` while active
    sign-out stamps ``127.0.0.1`` + now."""
    key = _broken_key(world, condition)
    client = _primed_client_with_cookie(signing.ADMIN_SESSION_COOKIE, key)
    res = client.post("/api/instances/admins/sign-out/", data={})
    assert res.status_code == 302, (condition, res.status_code, res.text[:200])
    ip, at = _logout_stamps(world["db"], "admin@example.com")
    assert ip == "", (condition, ip)
    assert at is None, (condition, at)


def test_fresh_session_passes_control(admin_api, world):
    """Control: the same forge for a live session stays authenticated.

    Guards a vacuous pass — a broken forge (wrong SECRET_KEY) would 401
    everywhere and make the broken-shape tests above pass for the wrong
    reason.
    """
    assert admin_api.get("/api/instances/admins/me/").status_code == 200
    body = admin_api.get("/api/instances/admins/session/").json()
    assert body["is_authenticated"] is True
    res = admin_api.patch("/api/instances/", json={"instance_name": "Fresh"})
    assert res.status_code == 200
