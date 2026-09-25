# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/email-check/ + /auth/spaces/email-check/ — AllowAny + throttle.

``status`` is ``MAGIC_CODE`` only for password-autoset users (or unknown
emails) while SMTP is configured and magic login enabled; otherwise
``CREDENTIAL``.
"""

import uuid

from _harness.checks import require_keys
from _harness.settings import database_url

from .conftest import _unique_email


def _check(anon, path, email):
    res = anon.post(path, json={"email": email})
    assert res.status_code == 200, f"{path}: {res.status_code} {res.text[:200]!r}"
    body = res.json()
    require_keys(body, ["existing", "status"], ctx=f"email-check {email}")
    assert isinstance(body["existing"], bool)
    assert body["status"] in ("MAGIC_CODE", "CREDENTIAL")
    return body


def test_known_credential_user(anon, user):
    assert _check(anon, "/auth/email-check/", user["email"]) == {
        "existing": True,
        "status": "CREDENTIAL",
    }


def test_unknown_email_reports_magic_code_when_smtp_configured(anon):
    assert _check(anon, "/auth/email-check/", _unique_email()) == {
        "existing": False,
        "status": "MAGIC_CODE",
    }


def test_autoset_user_reports_magic_code(anon, autoset_user):
    assert _check(anon, "/auth/email-check/", autoset_user["email"]) == {
        "existing": True,
        "status": "MAGIC_CODE",
    }


def test_unknown_email_reports_credential_when_smtp_off(anon, db):
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
        assert _check(anon, "/auth/email-check/", _unique_email()) == {
            "existing": False,
            "status": "CREDENTIAL",
        }
    finally:
        with db.connect() as conn:
            with conn.cursor() as cur:
                cur.execute("DELETE FROM instance_configurations WHERE key = 'EMAIL_HOST'")
            conn.commit()


def test_missing_email(anon):
    res = anon.post("/auth/email-check/", json={})
    assert res.status_code == 400
    assert res.json() == {"error_code": 5010, "error_message": "EMAIL_REQUIRED"}


def test_invalid_email(anon):
    res = anon.post("/auth/email-check/", json={"email": "not-an-email"})
    assert res.status_code == 400
    assert res.json() == {"error_code": 5005, "error_message": "INVALID_EMAIL"}


def test_space_mirrors_app(anon, user):
    assert _check(anon, "/auth/spaces/email-check/", user["email"]) == {
        "existing": True,
        "status": "CREDENTIAL",
    }
    assert _check(anon, "/auth/spaces/email-check/", _unique_email()) == {
        "existing": False,
        "status": "MAGIC_CODE",
    }


def test_space_missing_and_invalid_email(anon):
    res = anon.post("/auth/spaces/email-check/", json={})
    assert res.status_code == 400
    assert res.json()["error_code"] == 5010
    res = anon.post("/auth/spaces/email-check/", json={"email": "not-an-email"})
    assert res.status_code == 400
    assert res.json()["error_code"] == 5005


def test_email_lookup_is_case_insensitive(anon, user):
    assert _check(anon, "/auth/email-check/", user["email"].upper())["existing"] is True
