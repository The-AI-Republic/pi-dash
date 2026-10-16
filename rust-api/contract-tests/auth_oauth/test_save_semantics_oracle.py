# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""User.save() oracle pin for the github callback fix (PIDASHCONV-1081).

Django ``User.save()`` (``apps/api/pi_dash/db/models/user.py:169-187``)
backfills a blank ``display_name`` from the email local part, rotates
``token`` (64 hex) whenever ``token_updated_at`` is set, and flips
``is_staff`` for superusers. The github callback runs that save twice
(create at ``adapter/base.py:328``, login stamps at ``base.py:233``);
the Rust port missed all three until PIDASHCONV-1081.

A github-login end-to-end test cannot run hermetically on either
backend: the github token/userinfo URLs are hardcoded ``https://``
consts with no stub seam (see "Known oracle limits" in
``test_permissions.py``), and this suite boots with no provider
credentials by design (``conftest.oauth_unconfigured``). The github
path itself is pinned by the Rust unit tests in ``oauth_github.rs``
(``save_backfill_goldens``, ``rotated_token_shape``), which fail on
the pre-fix behavior.

What this module pins instead, on BOTH backends through the hermetic
email-password flow, is the oracle both ports implement: ``display_name``
backfill on user creation and 64-hex token rotation on login. If either
backend ever diverges from ``User.save()`` here, these go red.

Not covered (no hermetic flow on the Rust backend implements them, so
no both-backends contract test can pin them): blank-``display_name``
backfill on a *login* save, and the superuser ``is_staff`` flip. Both
are ported in the github fix by mirroring the verified gitlab sibling
(``oauth_gitlab.rs`` ``insert_user`` + ``save_user_data``).
"""

import os
import re
import sys
import uuid

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402

from _harness import auth, config, db, factory  # noqa: E402

HEX64 = re.compile(r"[0-9a-f]{64}")


def test_signup_backfills_display_name_from_email_local_part(client: httpx.Client):
    tag = f"oasave{uuid.uuid4().hex[:8]}"
    email = f"{tag}@example.com"
    try:
        token_response = client.get("/auth/get-csrf-token/")
        token_response.raise_for_status()
        resp = client.post(
            "/auth/sign-up/",
            data={"email": email, "password": "Fresh-Strong-Pass-9!x"},
            headers={
                "X-CSRFToken": token_response.json()["csrf_token"],
                "Referer": config.base_url(),
            },
        )
        assert resp.status_code == 302, (resp.status_code, resp.text[:200])
        row = db.fetchone(
            config.database_url(),
            "SELECT display_name FROM users WHERE email = %s",
            (email,),
        )
        assert row is not None, "signup created no users row"
        assert row["display_name"] == tag, row
    finally:
        factory.cleanup_run(tag)


def test_signin_rotates_token_to_64_hex(client: httpx.Client):
    tag = f"oasave{uuid.uuid4().hex[:8]}"
    email = f"{tag}@example.com"
    user = factory.create_user(email)
    try:
        before = db.fetchone(
            config.database_url(),
            "SELECT token, token_updated_at FROM users WHERE email = %s",
            (email,),
        )
        assert before["token"] == "", before
        auth.login_session_cookie(config.base_url(), user["email"], user["password"])
        after = db.fetchone(
            config.database_url(),
            "SELECT token, token_updated_at FROM users WHERE email = %s",
            (email,),
        )
        assert after["token_updated_at"] is not None, after
        assert HEX64.fullmatch(after["token"]), after["token"]
        assert after["token"] != before["token"]
    finally:
        factory.cleanup_run(tag)
