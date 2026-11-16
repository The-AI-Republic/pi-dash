# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: denied-permission cases.

An expired or invalid magic code MUST NOT create a session: no
``session-id`` cookie on the response, no new ``sessions`` row for the user,
and the same client still gets 401 from the authenticated probe
``GET /api/users/me/``.
"""

import httpx
import pytest

from _harness import env, redis as redis_helper

from .conftest import (
    SESSION_COOKIE,
    csrf_client,
    fresh_email,
    generate,
    location_query,
    post_form,
    session_rows,
)

SIGN_IN = "/auth/magic-sign-in/"
GENERATE = "/auth/magic-generate/"


@pytest.mark.contract
class TestMagicDenied:
    @pytest.mark.parametrize("setup", ["expired", "invalid"])
    def test_bad_code_creates_no_session(self, setup, db, seed, rdb):
        user = seed.user(email=fresh_email())
        client = csrf_client()
        if setup == "invalid":
            redis_helper.clear_magic(rdb, user["email"])
            assert generate(client, GENERATE, user["email"]).status_code == 200
            code = "000000"
        else:
            redis_helper.clear_magic(rdb, user["email"])
            code = "000000"
        before = session_rows(db, user["id"])

        resp = post_form(client, SIGN_IN, {"email": user["email"], "code": code})

        assert resp.status_code == 302, resp.text
        assert "error_code" in location_query(resp)
        assert SESSION_COOKIE not in resp.cookies
        assert SESSION_COOKIE not in client.cookies
        assert session_rows(db, user["id"]) == before

        probe = client.get("/api/users/me/")
        assert probe.status_code == 401, probe.text

    def test_anonymous_probe_is_401(self):
        client = httpx.Client(base_url=env.base_url(), timeout=30.0)
        assert client.get("/api/users/me/").status_code == 401
