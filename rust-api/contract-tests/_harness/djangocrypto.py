"""Stdlib-only replicas of the Django crypto needed to seed auth state.

Session cookies and machine-token hashes are written straight into
Postgres, so the suite reimplements the exact signing recipes Django
uses (verified against Django 4.2 ``django/core/signing.py``,
``django/utils/crypto.py`` and ``django/contrib/sessions``):

- session cookie: TimestampSigner over the JSON session dict, salt
  ``django.contrib.sessions.SessionStore``, sha256, zlib-compressed
  when shorter, ``separators=(",", ":")``.
- session auth hash: ``salted_hmac``/sha1 of the stored password string.
- machine-token hash: ``HMAC-SHA256(sha256("runner/pepper/" +
  SECRET_KEY), raw)`` (see ``pi_dash.runner.services.tokens``).
"""

import base64
import hashlib
import hmac
import json
import time
import zlib


def _b64_encode(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def _b62_encode(number: int) -> str:
    alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
    if number == 0:
        return "0"
    out = ""
    while number > 0:
        number, remainder = divmod(number, 62)
        out = alphabet[remainder] + out
    return out


def _salted_hmac(key_salt: str, value: str, secret: str, algorithm: str):
    hasher = getattr(hashlib, algorithm)
    key = hasher((key_salt + secret).encode()).digest()
    return hmac.new(key, msg=value.encode(), digestmod=hasher)


def session_auth_hash(password: str, secret: str) -> str:
    """``AbstractBaseUser.get_session_auth_hash`` for a stored password."""
    return _salted_hmac(
        "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash",
        password,
        secret,
        # NOTE: Django >= 4.1.8 / 4.2 uses sha256 here (not the salted_hmac
        # default sha1). Verified against the pinned Django 4.2.30 source.
        "sha256",
    ).hexdigest()


def encode_session(session_dict: dict, secret: str) -> str:
    """``SessionStore.encode``: signed (+compressed) session payload."""
    salt = "django.contrib.sessions.SessionStore"
    data = json.dumps(session_dict, separators=(",", ":")).encode("latin-1")
    compressed = zlib.compress(data)
    if len(compressed) < len(data) - 1:
        payload = "." + _b64_encode(compressed)
    else:
        payload = _b64_encode(data)
    stamped = "%s:%s" % (payload, _b62_encode(int(time.time())))
    sig = _b64_encode(
        _salted_hmac(salt + "signer", stamped, secret, "sha256").digest()
    )
    return "%s:%s" % (stamped, sig)


def login_session_cookie(user_id: str, password: str, secret: str) -> str:
    """Full cookie value that authenticates as ``user_id`` via sessions."""
    return encode_session(
        {
            "_auth_user_id": str(user_id),
            "_auth_user_backend": "django.contrib.auth.backends.ModelBackend",
            "_auth_user_hash": session_auth_hash(password, secret),
        },
        secret,
    )


def machine_token_hash(raw: str, secret: str) -> str:
    pepper = hashlib.sha256(("runner/pepper/" + secret).encode()).digest()
    return hmac.new(pepper, raw.encode(), hashlib.sha256).hexdigest()


def machine_token_fingerprint(raw: str) -> str:
    return hashlib.sha256(raw.encode()).hexdigest()[:12]
