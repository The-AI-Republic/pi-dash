# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: magic sign-in (app route).

Pins ``MagicSignInEndpoint`` (``views/app/magic.py``): 302 redirect shapes
for every error branch, and session establishment on success (``session-id``
cookie, ``sessions`` row, ``GET /api/users/me/`` 200).
"""

import httpx
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

SIGN_IN = "/auth/magic-sign-in/"
GENERATE = "/auth/magic-generate/"


def _seed_token(client, rdb, email: str) -> str:
    redis_helper.clear_magic(rdb, email)
    assert generate(client, GENERATE, email).status_code == 200
    return str(redis_helper.read_magic(rdb, email)["token"])


@pytest.mark.contract
class TestMagicSignIn:
    def test_missing_fields_redirects_required(self, seed):
        client = csrf_client()
        resp = post_form(client, SIGN_IN, {})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5085"
        assert query["error_message"] == "MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED"

    def test_unknown_user_redirects_does_not_exist(self):
        client = csrf_client()
        resp = post_form(client, SIGN_IN, {"email": fresh_email(), "code": "123456"})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5060"
        assert query["error_message"] == "USER_DOES_NOT_EXIST"

    def test_expired_code_redirects_expired(self, seed, rdb):
        user = seed.user(email=fresh_email())
        redis_helper.clear_magic(rdb, user["email"])
        client = csrf_client()

        resp = post_form(client, SIGN_IN, {"email": user["email"], "code": "123456"})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5095"
        assert query["error_message"] == "EXPIRED_MAGIC_CODE_SIGN_IN"
        assert SESSION_COOKIE not in resp.cookies

    def test_invalid_code_redirects_invalid(self, seed, rdb):
        user = seed.user(email=fresh_email())
        client = csrf_client()
        _seed_token(client, rdb, user["email"])

        resp = post_form(client, SIGN_IN, {"email": user["email"], "code": "000000"})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5090"
        assert query["error_message"] == "INVALID_MAGIC_CODE_SIGN_IN"
        assert SESSION_COOKIE not in resp.cookies

    def test_success_sets_session(self, db, seed, rdb):
        user = seed.user(email=fresh_email())
        client = csrf_client()
        token = _seed_token(client, rdb, user["email"])
        before = session_rows(db, user["id"])

        resp = post_form(client, SIGN_IN, {"email": user["email"], "code": token})

        assert resp.status_code == 302, resp.text
        assert "error_code" not in location_query(resp)
        assert SESSION_COOKIE in resp.cookies
        assert session_rows(db, user["id"]) == before + 1
        assert redis_helper.read_magic(rdb, user["email"]) is None

        probe = client.get("/api/users/me/")
        assert probe.status_code == 200, probe.text
        assert probe.json()["email"] == user["email"]

    def test_success_with_next_path(self, seed, rdb):
        user = seed.user(email=fresh_email())
        client = csrf_client()
        token = _seed_token(client, rdb, user["email"])

        resp = post_form(
            client, SIGN_IN, {"email": user["email"], "code": token, "next_path": "/workspaces"}
        )

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert "error_code" not in query
        assert query["next_path"] == "/workspaces"
