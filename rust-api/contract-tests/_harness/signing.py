# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Pure-stdlib replication of the Django bits the suite needs to seed rows.

Session encoding mirrors django.contrib.sessions.backends.db on Django 4.2
(TimestampSigner over zlib-compressed compact JSON, salt
"django.contrib.sessions.SessionStore"); the auth hash mirrors
AbstractBaseUser.get_session_auth_hash (salted HMAC-SHA256 of the stored
password field). Verified against GET /api/instances/admins/session/.
"""

import base64
import hashlib
import hmac
import json
import secrets
import string
import time
import zlib

SESSION_SALT = "django.contrib.sessions.SessionStore"
AUTH_HASH_SALT = "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash"
AUTH_BACKEND = "django.contrib.auth.backends.ModelBackend"
ADMIN_SESSION_COOKIE = "admin-session-id"

_B62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
SESSION_KEY_CHARS = string.ascii_lowercase + string.digits


def b62_encode(value):
    if value == 0:
        return "0"
    out = ""
    while value > 0:
        value, rest = divmod(value, 62)
        out = _B62[rest] + out
    return out


def b64_encode(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def salted_hmac(key_salt, value, secret):
    hasher = hashlib.sha256
    key = hasher(key_salt.encode() + secret.encode()).digest()
    return hmac.new(key, msg=value.encode(), digestmod=hasher).digest()


def make_password_hash(password, iterations=600000):
    salt = "".join(secrets.choice(string.ascii_letters + string.digits) for _ in range(22))
    dk = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), iterations)
    return f"pbkdf2_sha256${iterations}${salt}${base64.b64encode(dk).decode().strip()}"


def encode_session(session_dict, secret):
    raw = json.dumps(session_dict, separators=(",", ":")).encode("latin-1")
    compressed = zlib.compress(raw)
    if len(compressed) < len(raw) - 1:
        payload = "." + b64_encode(compressed)
    else:
        payload = b64_encode(raw)
    stamped = f"{payload}:{b62_encode(int(time.time()))}"
    sig = b64_encode(salted_hmac(SESSION_SALT + "signer", stamped, secret))
    return f"{stamped}:{sig}"


def session_payload(user_id, password_hash, secret, device_info=None):
    auth_hash = salted_hmac(AUTH_HASH_SALT, password_hash, secret).hex()
    return {
        "_auth_user_id": str(user_id),
        "_auth_user_backend": AUTH_BACKEND,
        "_auth_user_hash": auth_hash,
        "device_info": device_info or {"user_agent": "contract-suite", "ip_address": "127.0.0.1", "domain": "contract"},
    }


def new_session_key():
    return "".join(secrets.choice(SESSION_KEY_CHARS) for _ in range(128))
