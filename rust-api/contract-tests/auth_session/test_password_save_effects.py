# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Password writes apply the ``User.save()`` column effects (PIDASHCONV-1352).

``change-password`` / ``set-password`` (``common.py:92-94`` / ``:131-133``)
and both ``reset-password`` twins funnel through a full ``user.save()``
(``db/models/user.py:169-187``), which on top of the password write:

- bumps ``updated_at`` (``auto_now``);
- rotates ``token`` / ``token_updated_at`` whenever ``token_updated_at``
  is already set (PB-1079);
- fills an empty ``display_name`` from the email prefix;
- sets ``is_staff`` for superusers.

Plus one quirk the tests pin: on ``set-password`` the 200 body shows a
*second* rotation that is never persisted — ``user_login`` fires
``update_last_login``, whose ``save(update_fields=["last_login"])``
runs the overridden ``save()`` body in memory and the serializer then
reads the in-memory user. So ``body.token != row.token`` on Django,
and Rust mirrors it.
"""

import psycopg

from _harness.db import Database
from _harness.settings import database_url, secret_key

from . import tokens
from .conftest import NEW_PASSWORD, PASSWORD, _unique_email, login_cookies, make_user

COLS = "updated_at, token, token_updated_at, display_name, is_staff, is_superuser"


def _row(uid: str) -> dict:
    with psycopg.connect(database_url()) as conn:
        with conn.cursor() as cur:
            cur.execute(f"SELECT {COLS} FROM users WHERE id = %s", (uid,))
            values = cur.fetchone()
    return dict(zip([c.strip() for c in COLS.split(",")], values))


def _rotating_user(db: Database, *, autoset: bool = False, superuser: bool = False) -> dict:
    """Seed a user whose ``token_updated_at`` is set (like a login leaves)."""
    user = make_user(db, _unique_email(), autoset=autoset)
    with psycopg.connect(database_url()) as conn:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET token = %s, token_updated_at = now() - interval '1 hour',"
                " is_superuser = %s WHERE id = %s",
                ("ef" * 32, superuser, user["id"]),
            )
        conn.commit()
    return user


def _assert_save_effects(before: dict, after: dict, *, rotated: bool):
    assert after["updated_at"] != before["updated_at"], "save() must bump updated_at"
    if rotated:
        assert after["token"] != before["token"], "save() must rotate token"
        assert len(after["token"]) == 64, after["token"]
        assert after["token_updated_at"] != before["token_updated_at"]
    else:
        assert after["token"] == before["token"]
        assert after["token_updated_at"] == before["token_updated_at"]
    assert after["display_name"] != "", "save() must fill an empty display_name"
    if after["is_superuser"]:
        assert after["is_staff"] is True


def test_set_password_applies_save_effects(browser, db):
    user = _rotating_user(db, autoset=True)
    before = _row(user["id"])
    res = browser(login_cookies(user)).post("/auth/set-password/", json={"password": NEW_PASSWORD})
    assert res.status_code == 200, res.text[:200]
    _assert_save_effects(before, _row(user["id"]), rotated=True)
    body = res.json()
    assert body["updated_at"] != before["updated_at"].isoformat().replace("+00:00", "Z")
    assert body["token"] != before["token"] and len(body["token"]) == 64
    assert body["token_updated_at"] != before["token_updated_at"].isoformat().replace("+00:00", "Z")
    # Phantom second rotation (see module docstring): the body token was
    # never persisted.
    assert body["token"] != _row(user["id"])["token"]
    assert body["display_name"] == _row(user["id"])["display_name"]


def test_change_password_applies_save_effects(browser, db):
    user = _rotating_user(db)
    before = _row(user["id"])
    res = browser(login_cookies(user)).post(
        "/auth/change-password/",
        json={"old_password": PASSWORD, "new_password": NEW_PASSWORD},
    )
    assert res.status_code == 200, res.text[:200]
    _assert_save_effects(before, _row(user["id"]), rotated=True)


def test_reset_password_applies_save_effects(browser, base, db):
    user = _rotating_user(db)
    before = _row(user["id"])
    uidb64, token = tokens.mint_reset_token(database_url(), secret_key(), user["id"])
    res = browser().post(f"/auth/reset-password/{uidb64}/{token}/", data={"password": NEW_PASSWORD})
    assert res.status_code == 302, res.text[:200]
    _assert_save_effects(before, _row(user["id"]), rotated=True)


def test_space_reset_password_applies_save_effects(browser, space_base, db):
    user = _rotating_user(db)
    before = _row(user["id"])
    uidb64, token = tokens.mint_reset_token(database_url(), secret_key(), user["id"])
    res = browser().post(
        f"/auth/spaces/reset-password/{uidb64}/{token}/", data={"password": NEW_PASSWORD}
    )
    assert res.status_code == 302, res.text[:200]
    _assert_save_effects(before, _row(user["id"]), rotated=True)


def test_no_rotation_without_token_updated_at(browser, db, autoset_user):
    before = _row(autoset_user["id"])
    assert before["token_updated_at"] is None
    res = browser(login_cookies(autoset_user)).post(
        "/auth/set-password/", json={"password": NEW_PASSWORD}
    )
    assert res.status_code == 200, res.text[:200]
    after = _row(autoset_user["id"])
    _assert_save_effects(before, after, rotated=False)
    assert after["token_updated_at"] is None
    body = res.json()
    assert body["token"] == before["token"] == ""
    assert body["token_updated_at"] is None


def test_superuser_gains_is_staff(browser, db):
    user = _rotating_user(db, autoset=True, superuser=True)
    before = _row(user["id"])
    assert before["is_staff"] is False
    res = browser(login_cookies(user)).post("/auth/set-password/", json={"password": NEW_PASSWORD})
    assert res.status_code == 200, res.text[:200]
    after = _row(user["id"])
    assert after["is_staff"] is True
    assert res.json()["is_staff"] is True
