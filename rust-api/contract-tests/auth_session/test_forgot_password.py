# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/forgot-password/ + /auth/spaces/forgot-password/.

AllowAny + throttle; needs EMAIL_HOST (DB-backed ``instance_configurations``
overrides the server env default) or the View answers SMTP_NOT_CONFIGURED.
A known user gets 200 + message (the reset email is enqueued for the
worker — Django's broker, the Rust Postgres queue since PIDASHCONV-1103;
no worker consumes it in the oracle), an unknown user 400.
"""

import os
import uuid

import pytest
import redis

from _harness.throttle import seed_throttle_window

from .conftest import _unique_email

DENIED_BODY = '{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}'


def test_known_user(anon, user):
    res = anon.post("/auth/forgot-password/", json={"email": user["email"]})
    assert res.status_code == 200, res.text[:200]
    assert res.json() == {"message": "Check your email to reset your password"}


def test_unknown_user(anon):
    res = anon.post("/auth/forgot-password/", json={"email": _unique_email()})
    assert res.status_code == 400
    assert res.json() == {"error_code": 5060, "error_message": "USER_DOES_NOT_EXIST"}


def test_invalid_email(anon):
    res = anon.post("/auth/forgot-password/", json={"email": "not-an-email"})
    assert res.status_code == 400
    assert res.json() == {"error_code": 5005, "error_message": "INVALID_EMAIL"}


def test_missing_email_is_invalid_email(anon):
    # No EMAIL_REQUIRED branch here (unlike email-check/): a missing email
    # fails validate_email and answers INVALID_EMAIL.
    res = anon.post("/auth/forgot-password/", json={})
    assert res.status_code == 400
    assert res.json() == {"error_code": 5005, "error_message": "INVALID_EMAIL"}


def test_nul_byte_email_is_invalid_email(anon):
    # PIDASHCONV-1353: the INVALID_EMAIL gate must run before the user
    # lookup — Postgres rejects a NUL byte in the SELECT bind param,
    # which answered 500 instead of Django's 400.
    res = anon.post("/auth/forgot-password/", json={"email": "a\x00@example.com"})
    assert res.status_code == 400
    assert res.json() == {"error_code": 5005, "error_message": "INVALID_EMAIL"}


def test_space_nul_byte_email_is_invalid_email(anon):
    # Space twin of the above (same shared handler, same gate order).
    res = anon.post("/auth/spaces/forgot-password/", json={"email": "a\x00@example.com"})
    assert res.status_code == 400
    assert res.json() == {"error_code": 5005, "error_message": "INVALID_EMAIL"}


def test_smtp_not_configured(anon, db, user):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO instance_configurations (id, key, value, category,"
                " is_encrypted, created_at, updated_at)"
                " VALUES (%s, 'EMAIL_HOST', '', 'general', false, now(), now())",
                (str(uuid.uuid4()),),
            )
        conn.commit()
    try:
        res = anon.post("/auth/forgot-password/", json={"email": user["email"]})
        assert res.status_code == 400
        assert res.json() == {"error_code": 5025, "error_message": "SMTP_NOT_CONFIGURED"}
    finally:
        with db.connect() as conn:
            with conn.cursor() as cur:
                cur.execute("DELETE FROM instance_configurations WHERE key = 'EMAIL_HOST'")
            conn.commit()


def test_space_known_user(anon, user):
    res = anon.post("/auth/spaces/forgot-password/", json={"email": user["email"]})
    assert res.status_code == 200, res.text[:200]
    assert res.json() == {"message": "Check your email to reset your password"}


def test_space_unknown_user(anon):
    res = anon.post("/auth/spaces/forgot-password/", json={"email": _unique_email()})
    assert res.status_code == 400
    assert res.json()["error_code"] == 5060


def test_space_smtp_not_configured(anon, db, user):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO instance_configurations (id, key, value, category,"
                " is_encrypted, created_at, updated_at)"
                " VALUES (%s, 'EMAIL_HOST', '', 'general', false, now(), now())",
                (str(uuid.uuid4()),),
            )
        conn.commit()
    try:
        res = anon.post("/auth/spaces/forgot-password/", json={"email": user["email"]})
        assert res.status_code == 400
        assert res.json() == {"error_code": 5025, "error_message": "SMTP_NOT_CONFIGURED"}
    finally:
        with db.connect() as conn:
            with conn.cursor() as cur:
                cur.execute("DELETE FROM instance_configurations WHERE key = 'EMAIL_HOST'")
            conn.commit()


@pytest.mark.parametrize("path", ["/auth/forgot-password/", "/auth/spaces/forgot-password/"])
def test_throttled_retry_after_is_ceiled(anon, path):
    # NEWPIDASH-16: the 429 carries Retry-After = ceil(wait). The seeded
    # window (30 timestamps 0.2s old) makes the true wait ~59.8s, so DRF
    # answers '60'; a floored renderer answers '59' and a missing header
    # fails outright.
    rdb = redis.Redis.from_url(os.environ["REDIS_URL"], socket_timeout=5)
    ident = f"fp16-{uuid.uuid4().hex}"
    seed_throttle_window(rdb, "authentication", ident)
    try:
        res = anon.post(
            path,
            json={"email": "n16-throttle@example.com"},
            headers={"X-Forwarded-For": ident},
        )
    finally:
        rdb.close()
    assert res.status_code == 429, res.text[:200]
    assert res.text == DENIED_BODY, res.text[:200]
    assert res.headers.get("retry-after") == "60", res.headers.get("retry-after")
