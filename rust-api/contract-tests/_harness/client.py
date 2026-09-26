# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""HTTP client fixtures shared by every contract-test domain, plus
authenticated clients for suites that forge Django db sessions.

The suite runs against a live backend (Django today, the Rust server
through the proxy tomorrow). ``BASE_URL`` selects the backend; redirects
are never followed so 3xx semantics stay observable.

Suites that authenticate with forged Django db sessions boot the backend
with a fixed ``SECRET_KEY`` (see ``README.md``), and the harness mints
``sessions`` rows with the exact Django 4.2 session encoding (JSON →
zlib → urlsafe-b64 → timestamp-signed with
``django.contrib.sessions.SessionStore`` salt, sha256). This exercises the
same ``SessionAuthentication`` path browsers use, without the sign-in
form's anonymous throttle — hundreds of per-test logins would otherwise
exhaust the 30/minute anon quota and 429 the suite.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
import secrets
import time
import zlib
from datetime import datetime, timedelta, timezone

import httpx
import pytest

from _harness.config import base_url as config_base_url
from _harness.db import db_cursor
from _harness.worlds import SeededUser


def get_base_url() -> str:
    try:
        return os.environ["BASE_URL"]
    except KeyError as exc:
        raise pytest.UsageError(
            "BASE_URL is not set (e.g. BASE_URL=http://127.0.0.1:8000)"
        ) from exc


@pytest.fixture(scope="session")
def base_url() -> str:
    return get_base_url().rstrip("/")


@pytest.fixture()
def client(base_url: str) -> httpx.Client:
    with httpx.Client(base_url=base_url, timeout=10, follow_redirects=False) as c:
        yield c

SESSION_SALT = "django.contrib.sessions.SessionStore"
SESSION_AUTH_SALT = (
    "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash"
)
SESSION_BACKEND = "django.contrib.auth.backends.ModelBackend"
SESSION_COOKIE = "session-id"
SESSION_AGE_SECONDS = 604800
_B62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def _contract_secret() -> str:
    try:
        return os.environ["CONTRACT_DJANGO_SECRET_KEY"]
    except KeyError:
        raise AssertionError(
            "CONTRACT_DJANGO_SECRET_KEY is not set — it must match the "
            "backend's SECRET_KEY (see rust-api/contract-tests/README.md)"
        ) from None


def _b62_encode(value: int) -> str:
    if value == 0:
        return "0"
    out = ""
    while value:
        value, rem = divmod(value, 62)
        out = _B62[rem] + out
    return out


def _sign_session(payload: dict, secret: str) -> str:
    # Mirrors TimestampSigner.sign_object(compress=True): the "." compression
    # marker prefixes the base64 *string* (and is covered by the signature),
    # not the raw bytes.
    raw = json.dumps(payload, separators=(",", ":")).encode("latin-1")
    compressed = zlib.compress(raw)
    if len(compressed) < (len(raw) - 1):
        value = "." + base64.urlsafe_b64encode(compressed).rstrip(b"=").decode()
    else:
        value = base64.urlsafe_b64encode(raw).rstrip(b"=").decode()
    stamp = _b62_encode(int(time.time()))
    derived = hashlib.sha256((SESSION_SALT + "signer" + secret).encode()).digest()
    sig = base64.urlsafe_b64encode(
        hmac.new(derived, f"{value}:{stamp}".encode(), hashlib.sha256).digest()
    ).rstrip(b"=").decode()
    return f"{value}:{stamp}:{sig}"


def _session_auth_hash(password_field: str, secret: str) -> str:
    # Mirrors AbstractBaseUser.get_session_auth_hash: Django 4.2+ flushes
    # sessions that lack this key, so forged sessions must carry it.
    derived = hashlib.sha256(
        (SESSION_AUTH_SALT + secret).encode()
    ).digest()
    return hmac.new(derived, password_field.encode(), hashlib.sha256).hexdigest()


def forge_session_key(user: SeededUser) -> str:
    """Insert a live session row for ``user``; return its cookie value."""
    secret = _contract_secret()
    key = "".join(secrets.choice("abcdefghijklmnopqrstuvwxyz0123456789") for _ in range(32))
    data = _sign_session(
        {
            "_auth_user_id": user.id,
            "_auth_user_backend": SESSION_BACKEND,
            "_auth_user_hash": _session_auth_hash(user.password_hash, secret),
        },
        secret,
    )
    expires = datetime.now(timezone.utc) + timedelta(seconds=SESSION_AGE_SECONDS)
    with db_cursor() as cur:
        cur.execute(
            "INSERT INTO sessions (session_key, session_data, expire_date, user_id)"
            " VALUES (%s,%s,%s,%s)",
            (key, data, expires, user.id),
        )
    return key


def login_client(user: SeededUser) -> httpx.Client:
    client = httpx.Client(base_url=config_base_url(), timeout=30.0)
    client.cookies.set(SESSION_COOKIE, forge_session_key(user))
    return client


def anonymous_client() -> httpx.Client:
    return httpx.Client(base_url=config_base_url(), timeout=30.0)
