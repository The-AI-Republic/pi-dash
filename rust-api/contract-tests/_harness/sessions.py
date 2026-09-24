"""Session-cookie minting with the standard library only.

The app API authenticates via the ``session-id`` cookie backed by the
``sessions`` table (custom engine ``pi_dash.db.models.session``). A row holds
a ``TimestampSigner``-signed JSON payload (zlib-compressed when shorter):
``b64(json):b62(now):b64(HMAC-SHA256)`` with salt
``"django.contrib.sessions.SessionStore" + "signer"``. The payload carries
``_auth_user_id`` / ``_auth_user_backend`` / ``_auth_user_hash`` where the
hash is ``HMAC-SHA256("...AbstractBaseUser.get_session_auth_hash",
password_field)``. Replicated here with hashlib/hmac so suites never import
Django; verified against the live backend (Django decodes our output).
"""

from __future__ import annotations

import base64
import datetime
import hashlib
import hmac
import json
import secrets
import string
import time
import zlib

from . import config

SESSION_COOKIE_NAME = "session-id"
SESSION_KEY_SALT = "django.contrib.sessions.SessionStore"
AUTH_HASH_SALT = "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash"
AUTH_BACKEND = "django.contrib.auth.backends.ModelBackend"

_B62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def _b64(b: bytes) -> str:
    return base64.urlsafe_b64encode(b).rstrip(b"=").decode()


def _b62(n: int) -> str:
    if n == 0:
        return "0"
    sign = "-" if n < 0 else ""
    n = abs(n)
    out = ""
    while n:
        n, r = divmod(n, 62)
        out = _B62[r] + out
    return sign + out


def _salted_hmac(key_salt: str, value: str, secret: str) -> hmac.HMAC:
    key = hashlib.sha256((key_salt + secret).encode()).digest()
    return hmac.new(key, msg=value.encode(), digestmod=hashlib.sha256)


def mint_session_data(session_dict: dict, secret: str) -> str:
    data = json.dumps(session_dict, separators=(",", ":")).encode("latin-1")
    compressed = zlib.compress(data)
    if len(compressed) < len(data) - 1:
        data = compressed
        prefix = "."
    else:
        prefix = ""
    value = prefix + _b64(data)
    stamped = "%s:%s" % (value, _b62(int(time.time())))
    sig = _b64(_salted_hmac(SESSION_KEY_SALT + "signer", stamped, secret).digest())
    return "%s:%s" % (stamped, sig)


def session_auth_hash(password_field: str, secret: str) -> str:
    return _salted_hmac(AUTH_HASH_SALT, password_field, secret).hexdigest()


def make_password_hash(password: str, iterations: int = 600_000) -> str:
    """Render a ``pbkdf2_sha256`` password field (Django format, stdlib)."""
    alphabet = string.ascii_letters + string.digits
    salt = "".join(secrets.choice(alphabet) for _ in range(22))
    dk = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), iterations)
    return "pbkdf2_sha256$%d$%s$%s" % (
        iterations,
        salt,
        base64.b64encode(dk).decode().strip(),
    )


def login(conn, user_id: str, password_field: str) -> dict:
    """Insert a session row for ``user_id``; return ``{cookie: key}`` for httpx."""
    secret = config.secret_key()
    payload = {
        "_auth_user_id": str(user_id),
        "_auth_user_backend": AUTH_BACKEND,
        "_auth_user_hash": session_auth_hash(password_field, secret),
    }
    key = secrets.token_hex(16)
    expire = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=7)
    conn.execute(
        "INSERT INTO sessions (session_key, session_data, expire_date, user_id)"
        " VALUES (%s, %s, %s, %s)",
        (key, mint_session_data(payload, secret), expire.isoformat(), str(user_id)),
    )
    return {SESSION_COOKIE_NAME: key}
