# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""At-rest encryption for BYOK LLM API keys.

Uses Fernet with key rotation via ``MultiFernet`` (first key encrypts, all keys
decrypt). ``ASSISTANT_ENCRYPTION_KEY`` is a comma-separated list of urlsafe
base64 32-byte keys. See ``.ai_design/integrate_ai_agent/02-backend.md`` §7.
"""

from __future__ import annotations

from cryptography.fernet import Fernet, InvalidToken, MultiFernet
from django.conf import settings

from pi_dash.assistant.errors import AssistantNotConfigured


def _fernet() -> MultiFernet:
    raw = getattr(settings, "ASSISTANT_ENCRYPTION_KEY", "") or ""
    keys = [k.strip() for k in raw.split(",") if k.strip()]
    if not keys:
        raise AssistantNotConfigured(
            "ASSISTANT_ENCRYPTION_KEY is not set; BYOK keys cannot be stored."
        )
    try:
        return MultiFernet([Fernet(k.encode()) for k in keys])
    except (ValueError, TypeError) as exc:  # malformed key material
        raise AssistantNotConfigured(f"ASSISTANT_ENCRYPTION_KEY is invalid: {exc}") from exc


def is_configured() -> bool:
    return bool((getattr(settings, "ASSISTANT_ENCRYPTION_KEY", "") or "").strip())


def encrypt(plaintext: str) -> bytes:
    return _fernet().encrypt(plaintext.encode("utf-8"))


def decrypt(token: bytes) -> str:
    if not token:
        return ""
    try:
        return _fernet().decrypt(bytes(token)).decode("utf-8")
    except InvalidToken as exc:
        raise AssistantNotConfigured(
            "Stored BYOK key could not be decrypted with the current key set."
        ) from exc


def rotate(token: bytes) -> bytes:
    """Re-encrypt a token under the primary key (used by the rotation command)."""
    return _fernet().rotate(bytes(token))
