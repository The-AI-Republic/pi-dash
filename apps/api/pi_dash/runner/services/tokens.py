# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Token minting + hashing + JWT helpers for the per-runner transport.

Three token kinds are minted server-side under the new HTTPS long-poll
transport (see ``.ai_design/move_to_https/design.md``):

- **Enrollment token** (``apd_en_…``) — short-lived, one-time. Created
  alongside a Runner row in PENDING state. Shown to the user once in
  the cloud UI as part of the runner-enroll install command.
- **Refresh token** (``rt_…``) — long-lived per-runner credential
  stored on disk at 0600. Rotated on every successful refresh.
- **Machine token** (``mt_…``) — separate machine-scoped CLI
  credential, independent of runner transport.

Access tokens are short-lived JWTs minted at refresh time and never
persisted on disk; see :func:`mint_access_token` /
:func:`decode_access_token`.
"""

from __future__ import annotations

import hashlib
import hmac
import secrets
import time
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone as _timezone
from typing import Any, Dict, Optional

import jwt
from django.conf import settings
from django.utils import timezone

ENROLLMENT_TTL = timedelta(hours=1)
ENROLLMENT_PREFIX = "apd_en_"
REFRESH_TOKEN_PREFIX = "rt_"
MACHINE_TOKEN_PREFIX = "mt_"
# Legacy connection secret prefix kept so old fixtures still hash the
# same way; no new secrets are minted with this prefix.
CONNECTION_SECRET_PREFIX = "apd_cs_"


def _pepper() -> bytes:
    return hashlib.sha256(("runner/pepper/" + settings.SECRET_KEY).encode()).digest()


def hash_token(raw: str) -> str:
    """HMAC-SHA256(pepper, raw) hex — stored alongside token rows."""
    return hmac.new(_pepper(), raw.encode(), hashlib.sha256).hexdigest()


def fingerprint(raw: str) -> str:
    """Short non-secret identifier for logs / admin UI."""
    return hashlib.sha256(raw.encode()).hexdigest()[:12]


@dataclass(frozen=True)
class MintedToken:
    raw: str
    hashed: str
    fingerprint: str


@dataclass(frozen=True)
class MintedEnrollment(MintedToken):
    expires_at: object


def mint_enrollment_token() -> MintedEnrollment:
    raw = ENROLLMENT_PREFIX + secrets.token_urlsafe(24)
    return MintedEnrollment(
        raw=raw,
        hashed=hash_token(raw),
        fingerprint=fingerprint(raw),
        expires_at=timezone.now() + ENROLLMENT_TTL,
    )


def mint_refresh_token() -> MintedToken:
    raw = REFRESH_TOKEN_PREFIX + secrets.token_urlsafe(32)
    return MintedToken(raw=raw, hashed=hash_token(raw), fingerprint=fingerprint(raw))


def mint_machine_token() -> MintedToken:
    raw = MACHINE_TOKEN_PREFIX + secrets.token_urlsafe(32)
    return MintedToken(raw=raw, hashed=hash_token(raw), fingerprint=fingerprint(raw))


def mint_connection_secret() -> MintedToken:
    """Legacy helper retained for tests that still reference Connection."""
    raw = CONNECTION_SECRET_PREFIX + secrets.token_urlsafe(32)
    return MintedToken(raw=raw, hashed=hash_token(raw), fingerprint=fingerprint(raw))


# ---- JWT access token (HS256) ------------------------------------------------

ACCESS_TOKEN_ALG = "HS256"
ACCESS_TOKEN_ISS = "pi-dash-cloud"


def _key_ring() -> Dict[str, Dict[str, str]]:
    """Return ``{kid: {secret, status}}`` from settings.

    ``settings.RUNNER_ACCESS_TOKEN_KEYS`` is a list of dicts so operators
    can roll keys without code changes. In dev (no setting present) we
    derive a single deterministic key from ``SECRET_KEY``.
    """
    keys = getattr(settings, "RUNNER_ACCESS_TOKEN_KEYS", None)
    if not keys:
        derived = hashlib.sha256(
            ("runner/access-token/" + settings.SECRET_KEY).encode()
        ).hexdigest()
        return {"default": {"secret": derived, "status": "active"}}
    return {
        entry["kid"]: {
            "secret": entry["secret"],
            "status": entry.get("status", "active"),
        }
        for entry in keys
    }


def _active_kid() -> str:
    for kid, meta in _key_ring().items():
        if meta.get("status") == "active":
            return kid
    raise RuntimeError("no active access-token signing key configured")


@dataclass(frozen=True)
class AccessToken:
    raw: str
    expires_at: datetime
    kid: str


def mint_access_token(
    *,
    runner_id: str,
    user_id: str,
    workspace_id: str,
    rtg: int,
    ttl_secs: Optional[int] = None,
) -> AccessToken:
    """Mint a per-runner access token.

    Payload follows ``design.md §5.2``:

    - ``sub`` is the runner id (string)
    - ``uid`` and ``wid`` carry the trust principal binding
    - ``rtg`` is the refresh-token generation that minted this token;
      verifiers reject if ``rtg < runner.refresh_token_generation - 1``
    """
    ttl = ttl_secs if ttl_secs is not None else int(
        getattr(settings, "ACCESS_TOKEN_TTL_SECS", 3600)
    )
    now = int(time.time())
    payload: Dict[str, Any] = {
        "iss": ACCESS_TOKEN_ISS,
        "sub": str(runner_id),
        "uid": str(user_id),
        "wid": str(workspace_id),
        "iat": now,
        "exp": now + ttl,
        "rtg": int(rtg),
    }
    kid = _active_kid()
    secret = _key_ring()[kid]["secret"]
    raw = jwt.encode(payload, secret, algorithm=ACCESS_TOKEN_ALG, headers={"kid": kid})
    return AccessToken(
        raw=raw,
        expires_at=datetime.fromtimestamp(now + ttl, tz=_timezone.utc),
        kid=kid,
    )


class AccessTokenError(Exception):
    """Raised when an access token fails verification."""

    def __init__(self, code: str, detail: str = "") -> None:
        super().__init__(detail or code)
        self.code = code
        self.detail = detail or code


def decode_access_token(raw: str) -> Dict[str, Any]:
    """Verify signature + ``exp`` and return the decoded payload.

    Caller is responsible for the ``rtg`` and revocation checks
    (``design.md §5.2`` step 3-4); those need a DB hit so they live
    outside this pure helper.

    Raises :class:`AccessTokenError` with codes:

    - ``access_token_malformed`` — header missing ``kid`` or unparseable
    - ``access_token_unknown_key`` — header ``kid`` not in the key ring
    - ``access_token_expired`` — ``exp`` is in the past
    - ``access_token_invalid`` — signature or claim shape wrong
    """
    try:
        header = jwt.get_unverified_header(raw)
    except jwt.PyJWTError as exc:
        raise AccessTokenError("access_token_malformed", str(exc)) from exc
    kid = header.get("kid")
    if not kid:
        raise AccessTokenError("access_token_malformed", "missing kid header")
    ring = _key_ring()
    meta = ring.get(kid)
    if meta is None:
        raise AccessTokenError("access_token_unknown_key", f"unknown kid {kid!r}")
    secret = meta["secret"]
    try:
        payload = jwt.decode(
            raw,
            secret,
            algorithms=[ACCESS_TOKEN_ALG],
            issuer=ACCESS_TOKEN_ISS,
            options={"require": ["exp", "iat", "sub", "uid", "wid", "rtg"]},
        )
    except jwt.ExpiredSignatureError as exc:
        raise AccessTokenError("access_token_expired", str(exc)) from exc
    except jwt.PyJWTError as exc:
        raise AccessTokenError("access_token_invalid", str(exc)) from exc
    return payload
