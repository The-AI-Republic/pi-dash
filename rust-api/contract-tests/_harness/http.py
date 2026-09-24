# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""HTTP client factories for contract suites."""

import base64
import hashlib
import hmac
import json
import secrets
import time
import zlib

import httpx

from . import config
from . import signing
from .settings import base_url


def make_client(cookies: dict | None = None) -> httpx.Client:
    """Generic client against ``BASE_URL`` (from ``_harness.config``)."""
    return httpx.Client(base_url=config.base_url(), cookies=cookies, timeout=30.0)


def anon_client(**kwargs):
    return httpx.Client(base_url=base_url(), timeout=15, **kwargs)


def admin_client(session_key, **kwargs):
    return httpx.Client(
        base_url=base_url(),
        cookies={signing.ADMIN_SESSION_COOKIE: session_key},
        timeout=15,
        **kwargs,
    )


def user_client(session_key, **kwargs):
    """App (`/api/...`) session auth: DRF SessionAuthentication over the
    `session-id` cookie (SESSION_COOKIE_NAME), same `sessions` rows as admin."""
    return httpx.Client(
        base_url=base_url(),
        cookies={signing.USER_SESSION_COOKIE: session_key},
        timeout=15,
        **kwargs,
    )


# The stock `anon` throttle is 30/min and only unauthenticated traffic counts
# against it — but the pre-login CSRF/sign-in hits plus the anon-client cases
# still burst past it over a full-file run. A 429 is the server asking us to
# wait, never a contract answer (no test pins a throttle shape), so ride it
# out with a bounded retry instead of failing the run.
_MAX_429_ATTEMPTS = 8


class ContractClient(httpx.Client):
    """httpx client that retries 429s, honoring Retry-After."""

    def request(self, method, url, **kwargs):  # type: ignore[override]
        response = super().request(method, url, **kwargs)
        for attempt in range(_MAX_429_ATTEMPTS - 1):
            if response.status_code != 429:
                return response
            retry_after = response.headers.get("retry-after")
            try:
                delay = max(float(retry_after), 1.0)  # type: ignore[arg-type]
            except (TypeError, ValueError):
                delay = 1.0 + attempt
            time.sleep(min(delay, 60.0))
            response = super().request(method, url, **kwargs)
        return response


# httpx client factories for the HTTP suites (space first use; extended by
# dispatch, PIDASHCONV-22). No Django test client anywhere near here.
def anonymous_client(base_url: str, timeout: float = 30.0) -> httpx.Client:
    """Unauthenticated client: the denied-permission and public-shape cases."""
    return ContractClient(base_url=base_url, timeout=timeout)


def api_client(base_url: str, timeout: float = 30.0, **kwargs) -> httpx.Client:
    """Authenticated-capable client with a cookie jar.

    Sessions come from ``login_session`` (a black-box sign-in POST), so the
    jar here is what carries the session cookie afterwards.
    """
    return ContractClient(base_url=base_url, timeout=timeout, follow_redirects=True, **kwargs)


# --- D-19 (PIDASHCONV-77) API-key helpers ---
# Union with the baseline above: ``base_url`` is the baseline's
# ``settings.base_url`` (identical semantics); the X-Api-Key client
# and asserting verbs below are added verbatim.

def client(api_key=None):
    headers = {}
    if api_key:
        headers["X-Api-Key"] = api_key
    return httpx.Client(base_url=base_url(), headers=headers, timeout=30)


def get(api_key, path, *, expect=200, params=None):
    with client(api_key) as c:
        r = c.get(path, params=params)
    assert r.status_code == expect, f"GET {path}: want {expect}, got {r.status_code}: {r.text[:400]!r}"
    return r


def post(api_key, path, *, expect=201, json=None):
    with client(api_key) as c:
        r = c.post(path, json=json)
    assert r.status_code == expect, f"POST {path}: want {expect}, got {r.status_code}: {r.text[:400]!r}"
    return r


def patch(api_key, path, *, expect=200, json=None):
    with client(api_key) as c:
        r = c.patch(path, json=json)
    assert r.status_code == expect, f"PATCH {path}: want {expect}, got {r.status_code}: {r.text[:400]!r}"
    return r


def delete(api_key, path, *, expect=204):
    with client(api_key) as c:
        r = c.delete(path)
    assert r.status_code == expect, (
        f"DELETE {path}: want {expect}, got {r.status_code}: {r.text[:400]!r}"
    )
    return r


# --- PIDASHCONV-83 (app project/state/estimate oracle) ---
# Union with the baseline above (see workpad for the full rationale).
SESSION_COOKIE_NAME = "session-id"
SESSION_KEY_SALT = "django.contrib.sessions.SessionStore"
AUTH_BACKEND = "django.contrib.auth.backends.ModelBackend"
AUTH_HASH_SALT = (
    "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash"
)
SESSION_AGE_DAYS = 7

_B62_ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def _b62_encode(value: int) -> str:
    if value == 0:
        return "0"
    out = ""
    while value > 0:
        value, remainder = divmod(value, 62)
        out = _B62_ALPHABET[remainder] + out
    return out


def _b64_encode(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def _salted_hmac(key_salt: str, value: str, secret: str):
    key = hashlib.sha256((key_salt + secret).encode()).digest()
    return hmac.new(key, msg=value.encode(), digestmod=hashlib.sha256)


def forge_session_data(session_dict: dict, secret: str) -> str:
    """Serialize + sign a session dict exactly like Django's SessionBase.encode."""
    data = json.dumps(session_dict, separators=(",", ":")).encode("latin-1")
    compressed = zlib.compress(data)
    if len(compressed) < (len(data) - 1):
        data = compressed
        base64d = "." + _b64_encode(data)
    else:
        base64d = _b64_encode(data)
    value = "%s:%s" % (base64d, _b62_encode(int(time.time())))
    sig = _b64_encode(_salted_hmac(SESSION_KEY_SALT + "signer", value, secret).digest())
    return "%s:%s" % (value, sig)


def session_auth_hash(password_db_value: str, secret: str) -> str:
    """Replicate ``AbstractBaseUser.get_session_auth_hash`` for a seeded user."""
    return _salted_hmac(AUTH_HASH_SALT, password_db_value, secret).hexdigest()


def login(conn, user: dict, secret: str | None = None) -> str:
    """Create a live session row for a seeded user; return the session key."""
    secret = secret or config.required(config.CONTRACT_SECRET_KEY)
    payload = {
        "_auth_user_id": str(user["id"]),
        "_auth_user_backend": AUTH_BACKEND,
        "_auth_user_hash": session_auth_hash(user["password"], secret),
    }
    session_key = secrets.token_hex(16)
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO sessions (session_key, session_data, expire_date)"
            " VALUES (%s, %s, now() + make_interval(days => %s))",
            (session_key, forge_session_data(payload, secret), SESSION_AGE_DAYS),
        )
    conn.commit()
    return session_key


def authed_client(base_url: str, session_key: str) -> httpx.Client:
    """httpx client carrying the forged session cookie."""
    return httpx.Client(
        base_url=base_url,
        cookies={SESSION_COOKIE_NAME: session_key},
        timeout=30.0,
    )


# --- PIDASHCONV-97 (runner daemon API): daemon wire-format clients ---
# Union with the baseline above: the runner daemon authenticates with
# bearer tokens and X-Api-Key headers (see runner/authentication.py),
# which no earlier suite needed.


def bearer_client(
    base_url: str, token: str, timeout: float = 30.0, headers: dict | None = None
) -> httpx.Client:
    """Client presenting ``Authorization: Bearer <token>``.

    This is the daemon wire format byte for byte: runner access tokens,
    runner refresh tokens (refresh endpoint) and ``mt_`` machine tokens all
    travel in this header (see ``runner/authentication.py``). Extra headers
    (e.g. ``X-Runner-Id``) ride along unchanged.
    """
    merged = {"Authorization": f"Bearer {token}"}
    if headers:
        merged.update(headers)
    return httpx.Client(base_url=base_url, timeout=timeout, headers=merged)


def api_key_client(base_url: str, token: str, timeout: float = 30.0) -> httpx.Client:
    """Client presenting ``X-Api-Key: <token>`` — the installed CLI path
    (``APIKeyAuthentication.auth_header_name``)."""
    return httpx.Client(
        base_url=base_url,
        timeout=timeout,
        headers={"X-Api-Key": token},
    )

