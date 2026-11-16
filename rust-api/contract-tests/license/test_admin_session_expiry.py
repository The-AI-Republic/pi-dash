# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance-admin session expiry survives re-saves (PIDASHCONV-1313).

Django's admin login calls ``set_expiry(ADMIN_SESSION_COOKIE_AGE)``, storing
``_session_expiry = 3600`` in the session payload, so every later save of
that session recomputes the row's ``expire_date`` as now + 1h. The Rust port
wrote no ``_session_expiry`` at login and never read it back, so the first
re-save pushed the row out to ``SESSION_COOKIE_AGE`` (7 days).

Sign in through the real form endpoint, assert the row and payload carry the
1-hour expiry, make a follow-up ``/api/instances/`` request, and assert the
re-saved row still carries it.

The re-save half discriminates only when the server under test runs with
``SESSION_SAVE_EVERY_REQUEST=1`` (a plain GET otherwise leaves the row
untouched on both backends); the ``_session_expiry`` payload assertions fail
on the unfixed Rust backend in every environment.

Runs unchanged against Django and the Rust port (``BASE_URL`` /
``DATABASE_URL`` / ``SECRET_KEY``); both backends must satisfy the same
absolute assertions.
"""

from __future__ import annotations

import time

from _harness import signing
from _harness.http import ContractClient
from _harness.settings import base_url, secret_key

ADMIN_COOKIE = signing.ADMIN_SESSION_COOKIE
ADMIN_AGE = 3600
# Login stamp and re-save stamp are both exactly now + 3600 on either
# backend; the window only absorbs slow CI, never the 7-day regression.
MIN_AGE = 3000
MAX_AGE = 4200


def _primed_client():
    """Browser-like client: CSRF cookie jar + X-CSRFToken header set."""
    client = ContractClient(base_url=base_url(), timeout=15, follow_redirects=False)
    res = client.get("/auth/get-csrf-token/")
    assert res.status_code == 200, f"CSRF priming failed: {res.status_code}"
    client.headers["X-CSRFToken"] = res.json()["csrf_token"]
    return client


def _cookie_value(res, name):
    for raw in res.headers.get_list("set-cookie"):
        if raw.startswith(name + "="):
            return raw.split(";", 1)[0].split("=", 1)[1]
    raise AssertionError(f"no {name} cookie in {res.headers.get_list('set-cookie')!r}")


def _read_session_row(db, key):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT session_data, EXTRACT(EPOCH FROM expire_date)::BIGINT"
                " FROM sessions WHERE session_key = %s",
                (key,),
            )
            row = cur.fetchone()
    assert row is not None, "sign-in left no sessions row"
    return signing.decode_session(row[0], secret_key()), row[1]


def _assert_one_hour_lifetime(payload, expire_unix):
    assert payload.get("_session_expiry") == ADMIN_AGE, payload
    remaining = expire_unix - time.time()
    assert MIN_AGE < remaining < MAX_AGE, remaining


def test_admin_session_keeps_one_hour_expiry_across_resave(db):
    instance = db.make_instance()
    admin = db.make_user("expiry-admin@example.com")
    db.make_admin(instance["id"], admin["id"])

    client = _primed_client()
    res = client.post(
        "/api/instances/admins/sign-in/",
        data={"email": admin["email"], "password": "ContractPass123!"},
    )
    assert res.status_code == 302, (res.status_code, res.text[:300])
    key = _cookie_value(res, ADMIN_COOKIE)

    payload, expire_unix = _read_session_row(db, key)
    _assert_one_hour_lifetime(payload, expire_unix)

    res = client.get(
        "/api/instances/admins/session/",
        headers={"Cookie": f"{ADMIN_COOKIE}={key}"},
    )
    assert res.status_code == 200, (res.status_code, res.text[:300])

    payload, expire_unix = _read_session_row(db, key)
    _assert_one_hour_lifetime(payload, expire_unix)
