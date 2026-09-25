# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: spaces magic routes.

Pins ``MagicGenerateSpaceEndpoint`` / ``MagicSignInSpaceEndpoint`` /
``MagicSignUpSpaceEndpoint`` (``views/space/magic.py``). Same code families
as the app routes; success redirects land under the spaces base instead of
the app base.
"""

import pytest

from _harness import redis as redis_helper

from .conftest import (
    SESSION_COOKIE,
    csrf_client,
    fresh_email,
    generate,
    location_path,
    location_query,
    post_form,
    session_rows,
)

SPACE_GENERATE = "/auth/spaces/magic-generate/"
SPACE_SIGN_IN = "/auth/spaces/magic-sign-in/"
SPACE_SIGN_UP = "/auth/spaces/magic-sign-up/"


def _seed_token(client, rdb, email: str) -> str:
    redis_helper.clear_magic(rdb, email)
    assert generate(client, SPACE_GENERATE, email).status_code == 200
    return str(redis_helper.read_magic(rdb, email)["token"])


@pytest.mark.contract
class TestSpaceMagic:
    def test_space_sign_in_missing_fields(self):
        client = csrf_client()
        resp = post_form(client, SPACE_SIGN_IN, {})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5085"
        assert query["error_message"] == "MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED"

    def test_space_sign_in_unknown_user(self):
        client = csrf_client()
        resp = post_form(client, SPACE_SIGN_IN, {"email": fresh_email(), "code": "1"})

        assert resp.status_code == 302, resp.text
        assert location_query(resp)["error_message"] == "USER_DOES_NOT_EXIST"

    def test_space_sign_in_expired_code(self, seed, rdb):
        user = seed.user(email=fresh_email())
        redis_helper.clear_magic(rdb, user["email"])
        client = csrf_client()

        resp = post_form(client, SPACE_SIGN_IN, {"email": user["email"], "code": "1"})

        assert resp.status_code == 302, resp.text
        assert location_query(resp)["error_message"] == "EXPIRED_MAGIC_CODE_SIGN_IN"
        assert SESSION_COOKIE not in resp.cookies

    def test_space_sign_in_invalid_code(self, seed, rdb):
        user = seed.user(email=fresh_email())
        client = csrf_client()
        _seed_token(client, rdb, user["email"])

        resp = post_form(client, SPACE_SIGN_IN, {"email": user["email"], "code": "1"})

        assert resp.status_code == 302, resp.text
        assert location_query(resp)["error_message"] == "INVALID_MAGIC_CODE_SIGN_IN"
        assert SESSION_COOKIE not in resp.cookies

    def test_space_sign_in_success_lands_on_spaces_base(self, db, seed, rdb):
        user = seed.user(email=fresh_email())
        client = csrf_client()
        token = _seed_token(client, rdb, user["email"])

        resp = post_form(client, SPACE_SIGN_IN, {"email": user["email"], "code": token})

        assert resp.status_code == 302, resp.text
        assert "error_code" not in location_query(resp)
        assert location_path(resp).rstrip("/").endswith("/spaces")
        assert SESSION_COOKIE in resp.cookies
        assert session_rows(db, user["id"]) >= 1

    def test_space_sign_up_missing_fields(self):
        client = csrf_client()
        resp = post_form(client, SPACE_SIGN_UP, {})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5055"
        assert query["error_message"] == "MAGIC_SIGN_UP_EMAIL_CODE_REQUIRED"

    def test_space_sign_up_existing_user(self, seed):
        user = seed.user(email=fresh_email())
        client = csrf_client()

        resp = post_form(client, SPACE_SIGN_UP, {"email": user["email"], "code": "1"})

        assert resp.status_code == 302, resp.text
        assert location_query(resp)["error_message"] == "USER_ALREADY_EXIST"

    def test_space_sign_up_expired_and_invalid(self, rdb):
        email = fresh_email()
        client = csrf_client()
        redis_helper.clear_magic(rdb, email)

        resp = post_form(client, SPACE_SIGN_UP, {"email": email, "code": "1"})
        assert resp.status_code == 302, resp.text
        assert location_query(resp)["error_message"] == "EXPIRED_MAGIC_CODE_SIGN_UP"

        _seed_token(client, rdb, email)
        resp = post_form(client, SPACE_SIGN_UP, {"email": email, "code": "1"})
        assert resp.status_code == 302, resp.text
        assert location_query(resp)["error_message"] == "INVALID_MAGIC_CODE_SIGN_UP"
        assert SESSION_COOKIE not in resp.cookies

    def test_space_sign_up_success_creates_user(self, db, seed, rdb):
        email = fresh_email()
        client = csrf_client()
        token = _seed_token(client, rdb, email)

        resp = post_form(client, SPACE_SIGN_UP, {"email": email, "code": token})

        assert resp.status_code == 302, resp.text
        assert "error_code" not in location_query(resp)
        assert location_path(resp).rstrip("/").endswith("/spaces")
        assert SESSION_COOKIE in resp.cookies

        with db.cursor() as cur:
            cur.execute("SELECT id FROM users WHERE email = %s", (email,))
            row = cur.fetchone()
        assert row is not None
        seed.track("users", "id", str(row[0]))
