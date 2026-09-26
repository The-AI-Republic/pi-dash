# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: magic sign-up (app route).

Pins ``MagicSignUpEndpoint`` (``views/app/magic.py``): same redirect shape
as sign-in with the ``SIGN_UP`` code family, and ``User``-row creation plus
session establishment on success.
"""

import pytest

from _harness import redis as redis_helper

from .conftest import (
    SESSION_COOKIE,
    csrf_client,
    fresh_email,
    generate,
    location_query,
    post_form,
    session_rows,
)

SIGN_UP = "/auth/magic-sign-up/"
GENERATE = "/auth/magic-generate/"


def _seed_token(client, rdb, email: str) -> str:
    redis_helper.clear_magic(rdb, email)
    assert generate(client, GENERATE, email).status_code == 200
    return str(redis_helper.read_magic(rdb, email)["token"])


@pytest.mark.contract
class TestMagicSignUp:
    def test_missing_fields_redirects_required(self):
        client = csrf_client()
        resp = post_form(client, SIGN_UP, {})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5055"
        assert query["error_message"] == "MAGIC_SIGN_UP_EMAIL_CODE_REQUIRED"

    def test_existing_user_redirects_already_exist(self, seed):
        user = seed.user(email=fresh_email())
        client = csrf_client()

        resp = post_form(client, SIGN_UP, {"email": user["email"], "code": "123456"})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5030"
        assert query["error_message"] == "USER_ALREADY_EXIST"

    def test_expired_code_redirects_expired(self, rdb):
        email = fresh_email()
        redis_helper.clear_magic(rdb, email)
        client = csrf_client()

        resp = post_form(client, SIGN_UP, {"email": email, "code": "123456"})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5097"
        assert query["error_message"] == "EXPIRED_MAGIC_CODE_SIGN_UP"
        assert SESSION_COOKIE not in resp.cookies

    def test_invalid_code_redirects_invalid(self, rdb):
        email = fresh_email()
        client = csrf_client()
        _seed_token(client, rdb, email)

        resp = post_form(client, SIGN_UP, {"email": email, "code": "000000"})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5092"
        assert query["error_message"] == "INVALID_MAGIC_CODE_SIGN_UP"
        assert SESSION_COOKIE not in resp.cookies

    def test_success_creates_user_and_sets_session(self, db, seed, rdb):
        email = fresh_email()
        client = csrf_client()
        token = _seed_token(client, rdb, email)

        with db.cursor() as cur:
            cur.execute("SELECT count(*) FROM users WHERE email = %s", (email,))
            assert cur.fetchone()[0] == 0

        resp = post_form(client, SIGN_UP, {"email": email, "code": token})

        assert resp.status_code == 302, resp.text
        assert "error_code" not in location_query(resp)
        assert SESSION_COOKIE in resp.cookies

        with db.cursor() as cur:
            cur.execute("SELECT id FROM users WHERE email = %s", (email,))
            row = cur.fetchone()
        assert row is not None
        user_id = str(row[0])
        seed.track("users", "id", user_id)
        assert session_rows(db, user_id) == 1

        probe = client.get("/api/users/me/")
        assert probe.status_code == 200, probe.text
        assert probe.json()["email"] == email
