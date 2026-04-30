# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Token minting + hashing for the connection auth flow.

Two token kinds are minted server-side:

- **Enrollment token** (``apd_en_…``) — short-lived, one-time. Created
  alongside a Connection row in PENDING state. Shown to the user once
  in the cloud UI as part of the ``pi-dash-runner connect`` install
  command. Exchanged by the daemon for a long-lived secret.
- **Connection secret** (``apd_cs_…``) — long-lived bearer used on
  every WebSocket connect (``Authorization: Bearer <secret>`` +
  ``X-Connection-Id``).
"""

from __future__ import annotations

import hashlib
import hmac
import secrets
from dataclasses import dataclass
from datetime import timedelta

from django.conf import settings
from django.utils import timezone

ENROLLMENT_TTL = timedelta(hours=1)
ENROLLMENT_PREFIX = "apd_en_"
CONNECTION_SECRET_PREFIX = "apd_cs_"


def _pepper() -> bytes:
    return hashlib.sha256(("runner/pepper/" + settings.SECRET_KEY).encode()).digest()


def hash_token(raw: str) -> str:
    """HMAC-SHA256(pepper, raw) hex — stored alongside connection rows."""
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


def mint_connection_secret() -> MintedToken:
    raw = CONNECTION_SECRET_PREFIX + secrets.token_urlsafe(32)
    return MintedToken(
        raw=raw,
        hashed=hash_token(raw),
        fingerprint=fingerprint(raw),
    )
