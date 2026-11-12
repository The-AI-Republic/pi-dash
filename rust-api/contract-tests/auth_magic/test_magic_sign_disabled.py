# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: magic sign-in/up with magic login disabled (PIDASHCONV-1078).

Pins the ``MagicCodeProvider.__init__`` ``MAGIC_LINK_LOGIN_DISABLED`` gate
on the four sign routes: with ``ENABLE_MAGIC_LINK_LOGIN`` set to ``"0"`` in
``instance_configurations`` (db-sourced on both backends), every sign POST
302s with ``error_code=5016`` and the ``email`` param carrying the provider
key (``magic_<addr>``) — Django's ``payload={"email": str(key)}`` quirk
(``magic_code.py:43-48``), ported verbatim.
"""

import uuid

import pytest

from .conftest import csrf_client, fresh_email, location_query, post_form

APP_SIGN_IN = "/auth/magic-sign-in/"
APP_SIGN_UP = "/auth/magic-sign-up/"
SPACE_SIGN_IN = "/auth/spaces/magic-sign-in/"
SPACE_SIGN_UP = "/auth/spaces/magic-sign-up/"


@pytest.fixture
def magic_disabled(db):
    """Set ENABLE_MAGIC_LINK_LOGIN to "0", restoring prior rows after."""
    with db.cursor() as cur:
        cur.execute(
            "SELECT id, key, value, category, is_encrypted"
            " FROM instance_configurations"
            " WHERE key = 'ENABLE_MAGIC_LINK_LOGIN' AND deleted_at IS NULL"
        )
        saved = cur.fetchall()
        cur.execute(
            "DELETE FROM instance_configurations"
            " WHERE key = 'ENABLE_MAGIC_LINK_LOGIN'"
        )
        cur.execute(
            "INSERT INTO instance_configurations"
            " (id, key, value, category, is_encrypted, created_at, updated_at)"
            " VALUES (%s, 'ENABLE_MAGIC_LINK_LOGIN', '0',"
            " 'general', false, now(), now())",
            (str(uuid.uuid4()),),
        )
    try:
        yield
    finally:
        with db.cursor() as cur:
            cur.execute(
                "DELETE FROM instance_configurations"
                " WHERE key = 'ENABLE_MAGIC_LINK_LOGIN'"
            )
            for row_id, key, value, category, is_encrypted in saved:
                cur.execute(
                    "INSERT INTO instance_configurations"
                    " (id, key, value, category, is_encrypted,"
                    " created_at, updated_at)"
                    " VALUES (%s, %s, %s, %s, %s, now(), now())",
                    (row_id, key, value, category, is_encrypted),
                )


@pytest.mark.contract
class TestMagicSignDisabled:
    def test_sign_in_redirects_disabled_with_prefixed_email(self, seed, magic_disabled):
        user = seed.user(email=fresh_email())
        client = csrf_client()

        resp = post_form(client, APP_SIGN_IN, {"email": user["email"], "code": "123456"})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5016"
        assert query["error_message"] == "MAGIC_LINK_LOGIN_DISABLED"
        assert query["email"] == f"magic_{user['email']}"

    def test_sign_up_redirects_disabled_with_prefixed_email(self, magic_disabled):
        email = fresh_email()
        client = csrf_client()

        resp = post_form(client, APP_SIGN_UP, {"email": email, "code": "123456"})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5016"
        assert query["error_message"] == "MAGIC_LINK_LOGIN_DISABLED"
        assert query["email"] == f"magic_{email}"

    def test_space_sign_in_redirects_disabled_with_prefixed_email(
        self, seed, magic_disabled
    ):
        user = seed.user(email=fresh_email())
        client = csrf_client()

        resp = post_form(
            client, SPACE_SIGN_IN, {"email": user["email"], "code": "123456"}
        )

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5016"
        assert query["error_message"] == "MAGIC_LINK_LOGIN_DISABLED"
        assert query["email"] == f"magic_{user['email']}"

    def test_space_sign_up_redirects_disabled_with_prefixed_email(self, magic_disabled):
        email = fresh_email()
        client = csrf_client()

        resp = post_form(client, SPACE_SIGN_UP, {"email": email, "code": "123456"})

        assert resp.status_code == 302, resp.text
        query = location_query(resp)
        assert query["error_code"] == "5016"
        assert query["error_message"] == "MAGIC_LINK_LOGIN_DISABLED"
        assert query["email"] == f"magic_{email}"
