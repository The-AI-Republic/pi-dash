"""Shared HTTP-auth helpers (public flows only, no Django internals).

Two complementary mechanisms live here — keep both:

- Login flow: ``GET /auth/get-csrf-token/`` then form ``POST
  /auth/sign-in/``. Session cookies carry the ``Secure`` flag, so plain
  ``http://`` test runs cannot rely on a cookie jar to round-trip them:
  the jar stores them but will not send them back over cleartext. This
  module therefore manages cookies manually — it extracts ``session-id``
  from the login response and sends it back as an explicit ``Cookie``
  header, which works over both HTTP and HTTPS.
- Forged DB sessions: for DB-seeding domains, forge session rows
  straight into Postgres and hand the cookie to httpx (no sign-in
  round-trips). The app API uses cookie sessions
  (``SESSION_COOKIE_NAME="session-id"``, custom engine
  ``pi_dash.db.models.session`` storing rows in the ``sessions`` table).
  The construction below mirrors Django 4.2 byte-for-byte, using only
  the standard library (never import Django here):

  - payload: ``JSONSerializer`` (``json.dumps(separators=(",", ":"))``
    as latin-1) + zlib compression when it saves ≥1 byte (``"."`` prefix)
  - signature: ``TimestampSigner``/sha256 over ``"<b64>:<b62 time>"``
    with salt ``"django.contrib.sessions.SessionStore"`` +
    ``"signer"``, keyed by the server's ``SECRET_KEY``
  - ``_auth_user_hash``: sha256 HMAC of the user's ``password`` field
    value, salted with
    ``"django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash"``

  The server must therefore start with ``SECRET_KEY`` pinned to a fixed
  test-only value, and the suite must know the same value (``SECRET_KEY``
  in the suite's environment). A rotating server key invalidates every
  seeded session — that failure looks like universal 403s.

Flow (all public endpoints, identical on Django and the Rust port):
1. ``GET /auth/get-csrf-token/`` -> ``{"csrf_token": ...}`` + ``csrftoken`` cookie.
2. ``POST /auth/sign-in/`` (form) with the token -> 302 + ``session-id`` cookie.
"""
import base64
import hashlib
import hmac
import json
import os
import secrets
import time
import urllib.parse
import zlib
from datetime import datetime, timedelta, timezone

import httpx
import pytest

SESSION_COOKIE = "session-id"


def login_session_cookie(base_url: str, email: str, password: str) -> str:
    probe = httpx.Client(base_url=base_url, timeout=30)
    r = probe.get("/auth/get-csrf-token/")
    r.raise_for_status()
    token = r.json()["csrf_token"]
    csrf_cookie = r.cookies.get("csrftoken")
    assert csrf_cookie, "no csrftoken cookie from /auth/get-csrf-token/"
    body = urllib.parse.urlencode(
        {"email": email, "password": password, "csrfmiddlewaretoken": token}
    )
    r2 = httpx.post(
        f"{base_url}/auth/sign-in/",
        content=body,
        headers={
            "Cookie": f"csrftoken={csrf_cookie}",
            "Referer": f"{base_url}/",
            "Content-Type": "application/x-www-form-urlencoded",
        },
        follow_redirects=False,
        timeout=30,
    )
    assert r2.status_code == 302, f"login failed: {r2.status_code} {r2.text[:200]}"
    session_cookie = None
    for raw in r2.headers.get_list("set-cookie"):
        if raw.startswith(SESSION_COOKIE + "="):
            session_cookie = raw.split(";", 1)[0].split("=", 1)[1]
    assert session_cookie, f"no {SESSION_COOKIE} cookie in login response"
    return session_cookie


def api_client(base_url: str, session_cookie: str | None) -> httpx.Client:
    """Client sending the session cookie explicitly (see module docstring)."""
    headers = {"Cookie": f"{SESSION_COOKIE}={session_cookie}"} if session_cookie else {}
    return httpx.Client(base_url=base_url, timeout=30, headers=headers)


SESSION_COOKIE_NAME = "session-id"
SESSION_SALT = "django.contrib.sessions.SessionStore"
AUTH_BACKEND = "django.contrib.auth.backends.ModelBackend"
AUTH_HASH_SALT = (
    "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash"
)
SESSION_AGE_SECONDS = 1209600  # Django default SESSION_COOKIE_AGE (2 weeks)

_B62_ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def get_secret_key() -> str:
    try:
        return os.environ["SECRET_KEY"]
    except KeyError as exc:
        raise pytest.UsageError(
            "SECRET_KEY is not set: it must match the live backend's "
            "pinned test secret so forged sessions verify"
        ) from exc


def _force_bytes(value) -> bytes:
    if isinstance(value, bytes):
        return value
    return str(value).encode("utf-8")


def _b64_encode(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def _salted_hmac_sha256(key_salt: str, value, secret: str) -> hmac.HMAC:
    key = hashlib.sha256(_force_bytes(key_salt) + _force_bytes(secret)).digest()
    return hmac.new(key, msg=_force_bytes(value), digestmod=hashlib.sha256)


def session_auth_hash(password_field: str, secret: str) -> str:
    """Mirror of ``AbstractBaseUser._get_session_auth_hash``."""
    return _salted_hmac_sha256(AUTH_HASH_SALT, password_field, secret).hexdigest()


def _b62_encode(number: int) -> str:
    if number == 0:
        return "0"
    sign = "-" if number < 0 else ""
    number = abs(number)
    encoded = ""
    while number > 0:
        number, remainder = divmod(number, 62)
        encoded = _B62_ALPHABET[remainder] + encoded
    return sign + encoded


def encode_session(session_dict: dict, secret: str) -> str:
    """Mirror of ``SessionBase.encode`` (JSON + compress) under TimestampSigner."""
    data = json.dumps(session_dict, separators=(",", ":")).encode("latin-1")
    compressed = zlib.compress(data)
    if len(compressed) < len(data) - 1:
        payload = "." + _b64_encode(compressed)
    else:
        payload = _b64_encode(data)
    stamped = "%s:%s" % (payload, _b62_encode(int(time.time())))
    sig = _b64_encode(
        _salted_hmac_sha256(SESSION_SALT + "signer", stamped, secret).digest()
    )
    return "%s:%s" % (stamped, sig)


def forge_session_key(
    conn, *, user_id: str, password_field: str, secret: str
) -> str:
    """Insert a ``sessions`` row for the user; return the cookie value."""
    session_key = secrets.token_hex(64)[:128]
    session_data = encode_session(
        {
            "_auth_user_id": str(user_id),
            "_auth_user_backend": AUTH_BACKEND,
            "_auth_user_hash": session_auth_hash(password_field, secret),
        },
        secret,
    )
    expire_date = datetime.now(timezone.utc) + timedelta(seconds=SESSION_AGE_SECONDS)
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO sessions "
            "(session_key, session_data, expire_date, user_id, device_info) "
            "VALUES (%s, %s, %s, %s, NULL)",
            (session_key, session_data, expire_date, str(user_id)),
        )
    return session_key


def cookie_header(session_key: str) -> dict:
    return {"Cookie": "%s=%s" % (SESSION_COOKIE_NAME, session_key)}


def authed_client(base_url: str, session_key: str) -> httpx.Client:
    """Short-lived httpx client bearing one forged session cookie."""
    return httpx.Client(
        base_url=base_url,
        timeout=10,
        follow_redirects=False,
        headers=cookie_header(session_key),
    )
