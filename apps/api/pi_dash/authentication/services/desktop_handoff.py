# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Share the CLI's sign-in with the desktop app.

``pidash auth login`` receives a ``dhk_`` handoff key next to its machine
token. When the desktop app is signed out but the daemon is signed in, it
asks the CLI (``pidash __desktop handoff``) to trade that key for a
single-use, 60-second code, then opens the redeem URL in its webview,
which turns the code into an ordinary browser session.

Only hashes of keys and codes are stored. The private cloud overlay imports
these functions, so their names and signatures are part of a contract.
"""

from __future__ import annotations

import logging
import secrets
from datetime import timedelta
from typing import Optional

from django.db import transaction
from django.utils import timezone

from pi_dash.runner.models import DesktopHandoffCode, DesktopHandoffKey
from pi_dash.runner.services.tokens import fingerprint as token_fingerprint
from pi_dash.runner.services.tokens import hash_token

logger = logging.getLogger("pi_dash.auth.desktop_handoff")

HANDOFF_KEY_PREFIX = "dhk_"
HANDOFF_CODE_PREFIX = "dhc_"
HANDOFF_CODE_TTL_SECONDS = 60


def mint_handoff_key(*, user, dev_machine) -> str:
    """Mint a key for (user, dev_machine), revoking earlier active ones."""
    raw = HANDOFF_KEY_PREFIX + secrets.token_urlsafe(32)
    with transaction.atomic():
        DesktopHandoffKey.objects.filter(user=user, dev_machine=dev_machine, revoked_at__isnull=True).update(
            revoked_at=timezone.now()
        )
        DesktopHandoffKey.objects.create(
            user=user,
            dev_machine=dev_machine,
            key_hash=hash_token(raw),
            key_fingerprint=token_fingerprint(raw),
        )
    return raw


def create_handoff_code(*, raw_key: str, machine_fingerprint: Optional[str]) -> Optional[tuple[str, int]]:
    """Trade a live handoff key for a short-lived code.

    Returns ``(code, ttl_seconds)`` or ``None`` when the key is unknown,
    revoked, bound to a revoked machine or an inactive user, or when the
    key's machine has a fingerprint and ``machine_fingerprint`` differs.
    """
    if not raw_key or not raw_key.startswith(HANDOFF_KEY_PREFIX):
        return None
    key = (
        DesktopHandoffKey.objects.select_related("user", "dev_machine")
        .filter(key_hash=hash_token(raw_key), revoked_at__isnull=True)
        .first()
    )
    if key is None:
        return None
    machine = key.dev_machine
    if machine.revoked_at is not None or not key.user.is_active:
        return None
    if machine.fingerprint and (machine_fingerprint or "") != machine.fingerprint:
        logger.info("desktop_handoff.fingerprint_mismatch key=%s dev_machine=%s", key.key_fingerprint, machine.pk)
        return None

    raw_code = HANDOFF_CODE_PREFIX + secrets.token_urlsafe(32)
    now = timezone.now()
    DesktopHandoffCode.objects.create(
        key=key,
        user=key.user,
        code_hash=hash_token(raw_code),
        expires_at=now + timedelta(seconds=HANDOFF_CODE_TTL_SECONDS),
    )
    DesktopHandoffKey.objects.filter(pk=key.pk).update(last_used_at=now)
    return raw_code, HANDOFF_CODE_TTL_SECONDS


def consume_handoff_code(raw_code: str):
    """Consume a code atomically; return its user, or ``None``.

    Single use: the row is marked consumed under a row lock, so two
    concurrent redeems of the same code cannot both succeed.
    """
    if not raw_code:
        return None
    now = timezone.now()
    with transaction.atomic():
        row = (
            DesktopHandoffCode.objects.select_for_update()
            .select_related("user", "key")
            .filter(code_hash=hash_token(raw_code))
            .first()
        )
        if row is None or row.consumed_at is not None:
            return None
        row.consumed_at = now
        row.save(update_fields=["consumed_at"])
        if row.expires_at <= now or row.key.revoked_at is not None or not row.user.is_active:
            return None
        return row.user


def revoke_handoff_keys(*, user, dev_machine=None) -> int:
    """Revoke the user's active handoff keys (optionally for one machine)."""
    qs = DesktopHandoffKey.objects.filter(user=user, revoked_at__isnull=True)
    if dev_machine is not None:
        qs = qs.filter(dev_machine=dev_machine)
    return qs.update(revoked_at=timezone.now())
