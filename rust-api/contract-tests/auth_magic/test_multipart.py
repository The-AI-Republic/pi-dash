# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: multipart/form-data posts on the six magic routes (PIDASHCONV-1079).

Django accepts multipart on all six routes: the generate routes are DRF
``APIView`` with the default parser classes (JSON, Form, MultiPart), and
the sign routes are plain Views reading ``request.POST``. Pins: generate
200 + ``{"key"}`` + Redis row, sign-in/up success 302 with a valid code,
and the REQUIRED redirect only when the fields are truly absent.
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
    session_rows,
)

APP_GENERATE = "/auth/magic-generate/"
SPACE_GENERATE = "/auth/spaces/magic-generate/"

SIGN_FLOWS = [
    ("/auth/magic-sign-in/", APP_GENERATE, "5085", "MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED", True),
    ("/auth/magic-sign-up/", APP_GENERATE, "5055", "MAGIC_SIGN_UP_EMAIL_CODE_REQUIRED", True),
    (
        "/auth/spaces/magic-sign-in/",
        SPACE_GENERATE,
        "5085",
        "MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED",
        False,
    ),
    (
        "/auth/spaces/magic-sign-up/",
        SPACE_GENERATE,
        "5055",
        "MAGIC_SIGN_UP_EMAIL_CODE_REQUIRED",
        False,
    ),
]


def post_multipart(client, path, fields, *, csrf_header=True, files=None):
    """POST ``fields`` as multipart/form-data with the client's CSRF secret.

    ``(None, value)`` tuples render as plain field parts (no filename);
    ``fields`` may be a dict or a list of pairs (for duplicate names).
    """
    items = list(fields.items()) if isinstance(fields, dict) else list(fields)
    multipart = [(key, (None, value)) for key, value in items]
    if files:
        extra = list(files.items()) if isinstance(files, dict) else list(files)
        multipart.extend(extra)
    headers = {"X-CSRFToken": client.cookies["csrftoken"]} if csrf_header else {}
    return client.post(path, files=multipart, headers=headers, follow_redirects=False)


def _seed_token(client, generate_path, rdb, email: str) -> str:
    redis_helper.clear_magic(rdb, email)
    assert generate(client, generate_path, email).status_code == 200
    return str(redis_helper.read_magic(rdb, email)["token"])


@pytest.mark.contract
@pytest.mark.parametrize("path", [APP_GENERATE, SPACE_GENERATE])
class TestMultipartGenerate:
    def test_generate_multipart_returns_key(self, path, rdb):
        client = csrf_client()
        email = fresh_email()
        redis_helper.clear_magic(rdb, email)

        resp = post_multipart(client, path, {"email": email})

        assert resp.status_code == 200, resp.text
        assert resp.json() == {"key": "magic_" + email}

        stored = redis_helper.read_magic(rdb, email)
        assert stored is not None
        assert set(stored) == {"current_attempt", "email", "token"}
        assert stored["email"] == email
        assert stored["current_attempt"] == 0
        assert len(str(stored["token"])) == 6

    def test_generate_multipart_missing_email_is_500(self, path):
        # BUG (ported): no email field -> "" -> validate_email raises,
        # uncaught -> 500, same as the JSON/urlencoded shape.
        client = csrf_client()
        resp = post_multipart(client, path, {"note": "no email here"})
        assert resp.status_code == 500, resp.text

    def test_generate_multipart_last_email_wins(self, path, rdb):
        first, second = fresh_email("mp-first"), fresh_email("mp-second")
        redis_helper.clear_magic(rdb, first)
        redis_helper.clear_magic(rdb, second)
        client = csrf_client()

        resp = post_multipart(client, path, [("email", first), ("email", second)])

        assert resp.status_code == 200, resp.text
        assert resp.json() == {"key": "magic_" + second}
        assert redis_helper.read_magic(rdb, second) is not None
        assert redis_helper.read_magic(rdb, first) is None


@pytest.mark.contract
@pytest.mark.parametrize("sign_path,generate_path,code,slug,is_app", SIGN_FLOWS)
class TestMultipartSign:
    def test_multipart_missing_fields_redirects_required(
        self, sign_path, generate_path, code, slug, is_app
    ):
        client = csrf_client()
        resp = post_multipart(client, sign_path, {"note": "no credentials here"})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == code
        assert query["error_message"] == slug

    def test_multipart_success(self, sign_path, generate_path, code, slug, is_app, db, seed, rdb):
        client = csrf_client()
        if "sign-up" in sign_path:
            email = fresh_email("mp-up")
            token = _seed_token(client, generate_path, rdb, email)
            before = None
        else:
            user = seed.user(email=fresh_email("mp-in"))
            email = user["email"]
            token = _seed_token(client, generate_path, rdb, email)
            before = session_rows(db, user["id"])

        resp = post_multipart(client, sign_path, {"email": email, "code": token})

        assert resp.status_code == 302, resp.text
        assert "error_code" not in location_query(resp), resp.headers["location"]
        assert SESSION_COOKIE in resp.cookies
        assert redis_helper.read_magic(rdb, email) is None
        if "sign-up" in sign_path:
            with db.cursor() as cur:
                cur.execute("SELECT id FROM users WHERE email = %s", (email,))
                row = cur.fetchone()
            assert row is not None
            seed.track("users", "id", str(row[0]))
            assert session_rows(db, str(row[0])) == 1
        else:
            assert session_rows(db, user["id"]) == before + 1
        if is_app:
            probe = client.get("/api/users/me/")
            assert probe.status_code == 200, probe.text
            assert probe.json()["email"] == email
        else:
            assert location_path(resp).rstrip("/").endswith("/spaces")

    def test_multipart_file_parts_ignored(
        self, sign_path, generate_path, code, slug, is_app, db, seed, rdb
    ):
        if "sign-up" in sign_path:
            email = fresh_email("mp-file-up")
        else:
            email = seed.user(email=fresh_email("mp-file-in"))["email"]
        client = csrf_client()
        token = _seed_token(client, generate_path, rdb, email)

        resp = post_multipart(
            client,
            sign_path,
            {"email": email, "code": token},
            files={"avatar": ("a.bin", b"xxx", "application/octet-stream")},
        )

        assert resp.status_code == 302, resp.text
        assert "error_code" not in location_query(resp), resp.headers["location"]
        assert SESSION_COOKIE in resp.cookies
        if "sign-up" in sign_path:
            with db.cursor() as cur:
                cur.execute("SELECT id FROM users WHERE email = %s", (email,))
                row = cur.fetchone()
            assert row is not None
            seed.track("users", "id", str(row[0]))

    def test_multipart_csrf_field_token(
        self, sign_path, generate_path, code, slug, is_app, db, seed, rdb
    ):
        # The csrfmiddlewaretoken POST field carries the secret (no header):
        # Django's CsrfViewMiddleware reads it from request.POST, and the
        # PIDASHCONV-1077 Rust gate must read it from the same pairs.
        if "sign-up" in sign_path:
            email = fresh_email("mp-csrf-up")
        else:
            email = seed.user(email=fresh_email("mp-csrf-in"))["email"]
        client = csrf_client()
        token = _seed_token(client, generate_path, rdb, email)

        resp = post_multipart(
            client,
            sign_path,
            {"email": email, "code": token, "csrfmiddlewaretoken": client.cookies["csrftoken"]},
            csrf_header=False,
        )

        assert resp.status_code == 302, resp.text
        assert "error_code" not in location_query(resp), resp.headers["location"]
        assert SESSION_COOKIE in resp.cookies
        if "sign-up" in sign_path:
            with db.cursor() as cur:
                cur.execute("SELECT id FROM users WHERE email = %s", (email,))
                row = cur.fetchone()
            assert row is not None
            seed.track("users", "id", str(row[0]))
