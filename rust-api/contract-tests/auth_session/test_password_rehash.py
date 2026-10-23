# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/sign-in/ upgrades outdated PBKDF2 hashes (Django setter).

``django.contrib.auth.base_user.check_password`` passes a setter that
re-encodes with the default hasher and saves whenever the password is
correct and ``hasher.must_update`` (wrong iteration count or a salt
below 128 bits). Accounts carried over from older installs are upgraded
on their next login instead of keeping the weaker hash.
"""

import base64
import hashlib
import secrets
import string

from _harness.redirects import assert_success_redirect

from .conftest import NEW_PASSWORD, PASSWORD, reprime


def _outdated_hash(password: str, *, iterations: int = 260_000, salt_len: int = 22) -> str:
    """A valid but outdated hash: Django 3.2/4.0 iterations, or a short salt."""
    alphabet = string.ascii_letters + string.digits
    salt = "".join(secrets.choice(alphabet) for _ in range(salt_len))
    dk = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), iterations)
    return "pbkdf2_sha256$%d$%s$%s" % (iterations, salt, base64.b64encode(dk).decode().strip())


def _downgrade(db, user, **kwargs) -> str:
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET password = %s WHERE id = %s",
                (_outdated_hash(PASSWORD, **kwargs), user["id"]),
            )
        conn.commit()
    return _password_of(db, user["id"])


def _password_of(db, user_id: str) -> str:
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT password FROM users WHERE id = %s", (user_id,))
            return cur.fetchone()[0]


def test_signin_rewrites_outdated_iterations(browser, base, db, user):
    before = _downgrade(db, user)
    assert before.startswith("pbkdf2_sha256$260000$")

    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": PASSWORD})
    assert_success_redirect(res, base=base)

    after = _password_of(db, user["id"])
    assert after != before
    assert after.startswith("pbkdf2_sha256$600000$")
    assert len(after.split("$")[2]) == 22

    # The same password still signs in (against the new hash now).
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": PASSWORD})
    assert_success_redirect(res, base=base)


def test_signin_rewrites_short_salt(browser, base, db, user):
    before = _downgrade(db, user, iterations=600_000, salt_len=12)
    assert before.startswith("pbkdf2_sha256$600000$")
    assert len(before.split("$")[2]) == 12

    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": PASSWORD})
    assert_success_redirect(res, base=base)

    after = _password_of(db, user["id"])
    assert after != before
    assert after.startswith("pbkdf2_sha256$600000$")
    assert len(after.split("$")[2]) == 22


def test_rehash_keeps_issued_session_valid(browser, db, user):
    # The session is issued after the setter save, so its auth hash covers
    # the NEW password field: the issued cookie must authenticate. (A
    # session hashed from the stale field would answer 401 here.)
    _downgrade(db, user)
    client = browser()
    res = client.post("/auth/sign-in/", data={"email": user["email"], "password": PASSWORD})
    assert res.status_code == 302
    reprime(client)
    res = client.post(
        "/auth/change-password/",
        json={"old_password": "Wrong-Pass-1!x", "new_password": NEW_PASSWORD},
    )
    assert res.status_code == 400
    assert res.json()["error_code"] == 5135
