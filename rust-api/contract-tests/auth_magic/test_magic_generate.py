# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: magic-link generation (app + spaces).

Pins ``MagicGenerateEndpoint`` / ``MagicGenerateSpaceEndpoint``
(``pi_dash/authentication/views/{app,space}/magic.py``): 200 ``{"key"}``
shape, Redis ``magic_<email>`` side effect, attempt-exhaustion 400s, and the
``INSTANCE_NOT_CONFIGURED`` gate. Recorded bug: empty/malformed emails 500
(``validate_email`` raises an uncaught Django ``ValidationError``) instead of
a 400 ``error_code``.
"""

import httpx
import pytest

from _harness import redis as redis_helper

from .conftest import csrf_client, fresh_email, generate

APP_GENERATE = "/auth/magic-generate/"
SPACE_GENERATE = "/auth/spaces/magic-generate/"


@pytest.mark.contract
@pytest.mark.parametrize("path", [APP_GENERATE, SPACE_GENERATE])
class TestMagicGenerate:
    def test_generate_returns_key_shape(self, path, rdb, seed):
        client = csrf_client()
        email = fresh_email()
        redis_helper.clear_magic(rdb, email)

        resp = generate(client, path, email)

        assert resp.status_code == 200, resp.text
        assert set(resp.json()) == {"key"}
        assert resp.json()["key"] == "magic_" + email

        stored = redis_helper.read_magic(rdb, email)
        assert stored is not None
        assert set(stored) == {"current_attempt", "email", "token"}
        assert stored["email"] == email
        assert stored["current_attempt"] == 0
        assert len(str(stored["token"])) == 6

    def test_generate_empty_email_is_500(self, path):
        # BUG (ported): validate_email("") raises django ValidationError,
        # uncaught by the view -> 500, not a 400 error_code.
        client = csrf_client()
        resp = generate(client, path, "")
        assert resp.status_code == 500, resp.text

    def test_generate_invalid_email_is_500(self, path):
        # BUG (ported): same as above for malformed addresses.
        client = csrf_client()
        resp = generate(client, path, "not-an-email")
        assert resp.status_code == 500, resp.text

    def test_generate_max_attempts_400_for_new_email(self, path, rdb):
        client = csrf_client()
        email = fresh_email()
        redis_helper.clear_magic(rdb, email)

        codes = [generate(client, path, email).status_code for _ in range(4)]
        assert codes == [200, 200, 200, 200]
        resp = generate(client, path, email)

        assert resp.status_code == 400, resp.text
        body = resp.json()
        assert set(body) == {"error_code", "error_message", "email"}
        assert body["error_code"] == 5102
        assert body["error_message"] == "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP"

    def test_generate_max_attempts_400_for_existing_user(self, path, rdb, seed):
        user = seed.user(email=fresh_email())
        client = csrf_client()
        redis_helper.clear_magic(rdb, user["email"])

        for _ in range(4):
            assert generate(client, path, user["email"]).status_code == 200
        resp = generate(client, path, user["email"])

        assert resp.status_code == 400, resp.text
        body = resp.json()
        assert body["error_code"] == 5100
        assert body["error_message"] == "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_IN"

    def test_generate_instance_not_configured(self, path, db):
        client = csrf_client()
        with db.cursor() as cur:
            cur.execute("UPDATE instances SET is_setup_done = false")
        try:
            resp = generate(client, path, fresh_email())
        finally:
            with db.cursor() as cur:
                cur.execute("UPDATE instances SET is_setup_done = true")

        assert resp.status_code == 400, resp.text
        assert resp.json() == {
            "error_code": 5000,
            "error_message": "INSTANCE_NOT_CONFIGURED",
        }
