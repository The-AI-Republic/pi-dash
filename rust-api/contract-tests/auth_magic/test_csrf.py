# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: CSRF enforcement on the six magic routes (PIDASHCONV-1077).

The four sign routes are plain Django ``View``s behind
``CsrfViewMiddleware``: a form POST without a valid token is rejected by
the custom ``csrf_failure`` view (HTTP 200 + the failure page) before the
view runs, so no session is created and the magic token is untouched. The
two generate routes are DRF ``APIView``s under ``SessionAuthentication``:
a session-authenticated caller without a valid token gets 403
``{"detail": "CSRF Failed: ..."}`` (auth runs before throttles, so the
request never reaches ``initiate()``); anonymous callers are unaffected.
"""

import httpx
import pytest

from _harness import env
from _harness import redis as redis_helper

from .conftest import (
    SESSION_COOKIE,
    csrf_client,
    fresh_email,
    generate,
    post_form,
    session_rows,
)

APP_GENERATE = "/auth/magic-generate/"
APP_SIGN_IN = "/auth/magic-sign-in/"
APP_SIGN_UP = "/auth/magic-sign-up/"
SPACE_GENERATE = "/auth/spaces/magic-generate/"
SPACE_SIGN_IN = "/auth/spaces/magic-sign-in/"
SPACE_SIGN_UP = "/auth/spaces/magic-sign-up/"

SIGN_PATHS = [APP_SIGN_IN, APP_SIGN_UP, SPACE_SIGN_IN, SPACE_SIGN_UP]
GENERATE_PATHS = [APP_GENERATE, SPACE_GENERATE]


def _seed_token(client, rdb, generate_path: str, email: str) -> str:
    redis_helper.clear_magic(rdb, email)
    assert generate(client, generate_path, email).status_code == 200
    return str(redis_helper.read_magic(rdb, email)["token"])


def _generate_for(sign_path: str) -> str:
    return SPACE_GENERATE if "/spaces/" in sign_path else APP_GENERATE


@pytest.mark.contract
@pytest.mark.parametrize("path", SIGN_PATHS)
class TestSignCsrf:
    def test_csrf_less_post_hits_failure_page(self, path, db, seed, rdb):
        user = seed.user(email=fresh_email())
        seeder = csrf_client()
        token = _seed_token(seeder, rdb, _generate_for(path), user["email"])
        before = session_rows(db, user["id"])

        bare = httpx.Client(base_url=env.base_url(), timeout=30.0)
        resp = bare.post(
            path,
            data={"email": user["email"], "code": token},
            follow_redirects=False,
        )

        assert resp.status_code == 200, resp.text
        assert "text/html" in resp.headers["content-type"]
        assert "CSRF Verification Failed" in resp.text
        assert "templates/csrf_failure.html" in resp.text
        assert SESSION_COOKIE not in resp.cookies
        assert SESSION_COOKIE not in bare.cookies
        assert session_rows(db, user["id"]) == before
        # The middleware rejects before the view runs: the token survives.
        assert redis_helper.read_magic(rdb, user["email"]) is not None

        probe = bare.get("/api/users/me/")
        assert probe.status_code == 401, probe.text


@pytest.mark.contract
@pytest.mark.parametrize("path", GENERATE_PATHS)
class TestGenerateCsrf:
    def _login(self, seed, rdb):
        user = seed.user(email=fresh_email())
        client = csrf_client()
        token = _seed_token(client, rdb, APP_GENERATE, user["email"])
        login = post_form(
            client, APP_SIGN_IN, {"email": user["email"], "code": token}
        )
        assert login.status_code == 302, login.text
        assert SESSION_COOKIE in client.cookies
        return user, client

    def test_session_without_csrf_is_403(self, path, seed, rdb):
        _, client = self._login(seed, rdb)

        authed = httpx.Client(base_url=env.base_url(), timeout=30.0)
        authed.cookies.set(SESSION_COOKIE, client.cookies[SESSION_COOKIE])

        email = fresh_email("ct1077")
        redis_helper.clear_magic(rdb, email)
        resp = authed.post(path, json={"email": email})

        assert resp.status_code == 403, resp.text
        assert "application/json" in resp.headers["content-type"]
        assert "cookie" in resp.headers.get("vary", "").lower()
        body = resp.json()
        assert set(body) == {"detail"}, body
        assert body["detail"].startswith("CSRF Failed: "), body
        # Auth runs before the handler: no code is generated.
        assert redis_helper.read_magic(rdb, email) is None

    def test_session_with_csrf_token_succeeds(self, path, seed, rdb):
        _, client = self._login(seed, rdb)

        email = fresh_email("ct1077")
        redis_helper.clear_magic(rdb, email)
        resp = client.post(
            path,
            json={"email": email},
            headers={"X-CSRFToken": client.cookies["csrftoken"]},
        )

        assert resp.status_code == 200, resp.text
        assert resp.json() == {"key": "magic_" + email}
