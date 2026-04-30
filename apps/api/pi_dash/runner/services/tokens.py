# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Token minting, hashing, and verification for runner auth.

A "registration token" is handed to a user one time and expires in one hour.
The runner presents it once to obtain a "runner_secret": a long-lived bearer
credential the daemon stores on disk and sends on every WS connect.
"""

from __future__ import annotations

import hashlib
import hmac
import secrets
from dataclasses import dataclass
from datetime import timedelta

from django.conf import settings
from django.utils import timezone

REGISTRATION_TTL = timedelta(hours=1)
# Display prefixes so users can visually distinguish token kinds.
REGISTRATION_PREFIX = "apd_reg_"
RUNNER_SECRET_PREFIX = "apd_rs_"
MACHINE_TOKEN_PREFIX = "apd_mt_"


def _pepper() -> bytes:
    # Derived from SECRET_KEY so rotating it invalidates all registration tokens.
    return hashlib.sha256(("runner/pepper/" + settings.SECRET_KEY).encode()).digest()


def hash_token(raw: str) -> str:
    """HMAC-SHA256(pepper, raw) hex — stored alongside runner rows."""
    return hmac.new(_pepper(), raw.encode(), hashlib.sha256).hexdigest()


def fingerprint(raw: str) -> str:
    """Short non-secret identifier (for logs / admin UI)."""
    return hashlib.sha256(raw.encode()).hexdigest()[:12]


@dataclass(frozen=True)
class MintedRegistration:
    raw: str
    hashed: str
    expires_at: object


def mint_registration_token() -> MintedRegistration:
    raw = REGISTRATION_PREFIX + secrets.token_urlsafe(24)
    return MintedRegistration(
        raw=raw,
        hashed=hash_token(raw),
        expires_at=timezone.now() + REGISTRATION_TTL,
    )


@dataclass(frozen=True)
class MintedRunnerSecret:
    raw: str
    hashed: str
    fingerprint: str


def mint_runner_secret() -> MintedRunnerSecret:
    raw = RUNNER_SECRET_PREFIX + secrets.token_urlsafe(32)
    return MintedRunnerSecret(
        raw=raw,
        hashed=hash_token(raw),
        fingerprint=fingerprint(raw),
    )


@dataclass(frozen=True)
class MintedMachineSecret:
    """A freshly-minted MachineToken bearer secret. The plaintext ``raw``
    is shown to the user once at creation and never persisted; the cloud
    keeps only ``hashed`` and ``fingerprint``.
    """

    raw: str
    hashed: str
    fingerprint: str


def mint_machine_token_secret() -> MintedMachineSecret:
    raw = MACHINE_TOKEN_PREFIX + secrets.token_urlsafe(32)
    return MintedMachineSecret(
        raw=raw,
        hashed=hash_token(raw),
        fingerprint=fingerprint(raw),
    )
