# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Magic sign-in applies the ``User.save()`` column effects (NEWPIDASH-41).

``save_user_data`` (``adapter/base.py:220-234``) ends in a full
``user.save()``, so the magic-code login — like the email login
(NEWPIDASH-17) — backfills an empty ``display_name`` from the email
prefix and sets ``is_staff`` for superusers
(``db/models/user.py:177-185``).
"""

import psycopg
import pytest

from _harness import redis as redis_helper
from _harness.settings import database_url

from .conftest import (
    csrf_client,
    fresh_email,
    generate,
    location_query,
    post_form,
)

CASES = [
    ("/auth/magic-generate/", "/auth/magic-sign-in/"),
    ("/auth/spaces/magic-generate/", "/auth/spaces/magic-sign-in/"),
]

COLS = "display_name, is_staff, is_superuser"


def _row(uid: str) -> dict:
    with psycopg.connect(database_url()) as conn:
        with conn.cursor() as cur:
            cur.execute(f"SELECT {COLS} FROM users WHERE id = %s", (uid,))
            values = cur.fetchone()
    return dict(zip([c.strip() for c in COLS.split(",")], values))


def _hollow_user(seed, email: str) -> dict:
    """Seed a superuser with an empty display name and no staff flag."""
    user = seed.user(email=email)
    with psycopg.connect(database_url()) as conn:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET display_name = '', is_superuser = true,"
                " is_staff = false WHERE id = %s",
                (user["id"],),
            )
        conn.commit()
    return user


@pytest.mark.contract
@pytest.mark.parametrize("generate_path,sign_in_path", CASES)
def test_magic_sign_in_applies_save_effects(seed, db, rdb, generate_path, sign_in_path):
    user = _hollow_user(seed, fresh_email("ct41"))
    before = _row(user["id"])
    assert before["display_name"] == ""
    assert before["is_superuser"] is True
    assert before["is_staff"] is False

    client = csrf_client()
    redis_helper.clear_magic(rdb, user["email"])
    assert generate(client, generate_path, user["email"]).status_code == 200
    token = str(redis_helper.read_magic(rdb, user["email"])["token"])

    resp = post_form(client, sign_in_path, {"email": user["email"], "code": token})

    assert resp.status_code == 302, resp.text
    assert "error_code" not in location_query(resp), resp.headers["location"]
    after = _row(user["id"])
    assert after["display_name"] == user["email"].split("@")[0], after
    assert after["is_staff"] is True, after
