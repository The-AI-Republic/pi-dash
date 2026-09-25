# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/forgot-password/ + /auth/spaces/forgot-password/.

AllowAny + throttle; needs EMAIL_HOST (DB-backed ``instance_configurations``
overrides the server env default) or the View answers SMTP_NOT_CONFIGURED.
A known user gets 200 + message (the reset email is enqueued to the
broker; no worker consumes it in the oracle), an unknown user 400.
"""

import uuid

from .conftest import _unique_email


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
