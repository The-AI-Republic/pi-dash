# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stdlib minting of ``PasswordResetTokenGenerator`` tokens (Django 4.2).

``generate_password_token`` (``views/app/password_management.py``) builds
``uidb64`` via ``urlsafe_base64_encode(smart_bytes(user.id))`` and the
token via ``PasswordResetTokenGenerator().make_token(user)``. The recipe
below mirrors Django 4.2 byte-for-byte with hashlib/hmac only, so the
suite can drive the reset-password Views without importing Django:

- ``ts``: seconds since 2001-01-01 (naive UTC now)
- ``login_ts``: ``""`` when ``last_login`` is NULL, else the naive
  timestamp without microseconds
- ``value``: ``f"{pk}{password_field}{login_ts}{ts}{email}"``
- ``token``: ``base36(ts) + "-" + salted_hmac_sha256(key_salt, value,
  SECRET_KEY).hexdigest()[::2]``
"""

import base64
import hashlib
import hmac
import uuid
from datetime import datetime, timezone

import psycopg

KEY_SALT = "django.contrib.auth.tokens.PasswordResetTokenGenerator"
_B36 = "0123456789abcdefghijklmnopqrstuvwxyz"


def _b36(number: int) -> str:
    if number == 0:
        return "0"
    out = ""
    while number:
        number, remainder = divmod(number, 36)
        out = _B36[remainder] + out
    return out


def _num_seconds() -> int:
    return int((datetime.now(timezone.utc).replace(tzinfo=None) - datetime(2001, 1, 1)).total_seconds())


def mint_reset_token(dsn: str, secret: str, user_id: str) -> tuple:
    """Return ``(uidb64, token)`` the server's ``check_token`` accepts."""
    with psycopg.connect(dsn) as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT password, last_login, email FROM users WHERE id = %s",
                (user_id,),
            )
            password_field, last_login, email = cur.fetchone()
    login_ts = "" if last_login is None else str(last_login.replace(microsecond=0, tzinfo=None))
    ts = _num_seconds()
    value = f"{user_id}{password_field}{login_ts}{ts}{email}"
    key = hashlib.sha256((KEY_SALT + secret).encode()).digest()
    digest = hmac.new(key, msg=value.encode(), digestmod=hashlib.sha256).hexdigest()[::2]
    uidb64 = base64.urlsafe_b64encode(str(user_id).encode()).rstrip(b"=").decode()
    return uidb64, f"{_b36(ts)}-{digest}"


def ghost_uidb64() -> str:
    """uidb64 for a well-formed UUID that names no user."""
    return base64.urlsafe_b64encode(str(uuid.uuid4()).encode()).rstrip(b"=").decode()


def non_uuid_uidb64() -> str:
    """uidb64 decoding to a non-UUID string (UUID field rejects it)."""
    return base64.urlsafe_b64encode(b"not-a-uuid").rstrip(b"=").decode()


def bad_utf8_uidb64() -> str:
    """uidb64 whose bytes are not valid UTF-8 (decode raises)."""
    return base64.urlsafe_b64encode(b"\xff\xfe\x00bad").rstrip(b"=").decode()
