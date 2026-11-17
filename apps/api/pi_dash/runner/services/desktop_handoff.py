# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Desktop session handoff: a CLI sign-in signs in the desktop app.

desktop_unified_signin §5. The daemon holds machine credentials, which are
not web sessions; the handoff turns one into the other once, through the
server, without exposing either credential to the app:

1. The app generates a verifier (32 random bytes, memory only) and asks
   the daemon over its local socket with ``challenge`` =
   base64url(SHA-256(verifier)).
2. The daemon calls ``POST /api/v1/auth/desktop-handoff/`` with one of its
   workspace daemon credentials and the challenge. The server stores a
   single-use code — random, stored hashed, bound to the user, the
   ``dev_machine`` and the challenge, valid 60 seconds — and returns it
   with the account it belongs to.
3. The app redeems the code with the verifier over the native transport
   (never a URL). The code alone is useless: redemption needs the
   verifier, which never leaves the app's process. On any mismatch the
   code is consumed and no session is created.

Every issue and redeem is audited with user, machine and outcome — never
the code. Issue and redeem are rate-limited per machine.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import logging
import secrets
from datetime import timedelta

from django.conf import settings as django_settings
from django.core.cache import cache
from django.db import transaction
from django.db.models import Q
from django.utils import timezone

from pi_dash.runner.models import DesktopHandoffCode
from pi_dash.runner.services.tokens import fingerprint, hash_token

logger = logging.getLogger("pi_dash.runner.desktop_handoff")

#: Single-use codes live 60 seconds (design §5).
HANDOFF_TTL = timedelta(seconds=60)
#: Rate limit: issues + redeems per machine per minute.
HANDOFF_RATE_LIMIT = 30
HANDOFF_RATE_WINDOW = timedelta(minutes=1)

HANDOFF_DISABLED = "handoff_disabled"


def handoff_enabled() -> bool:
    """Kill switch (design §5.1): default on. When off the endpoints refuse
    and the app falls back to a browser sign-in."""
    return bool(getattr(django_settings, "DESKTOP_SESSION_HANDOFF_ENABLED", True))


def challenge_for_verifier(verifier: bytes) -> str:
    """base64url(SHA-256(verifier)) — what the app sends as ``challenge``."""
    return base64.urlsafe_b64encode(hashlib.sha256(verifier).digest()).rstrip(b"=").decode("ascii")


def expected_challenge_for(verifier: bytes) -> str:
    """The challenge the server checks on redeem (same function, one name)."""
    return challenge_for_verifier(verifier)


def _rate_limited(machine_id, now) -> bool:
    """True when ``machine_id`` has exceeded the per-minute limit.

    ``cache.add`` creates the window's counter only when absent and
    ``cache.incr`` bumps it atomically, so concurrent requests cannot both
    read the same count and slip past the limit.
    """
    window = int(now.timestamp() // HANDOFF_RATE_WINDOW.total_seconds())
    key = f"desktop_handoff_rate:{machine_id}:{window}"
    cache.add(key, 0, timeout=int(HANDOFF_RATE_WINDOW.total_seconds()) + 1)
    try:
        count = cache.incr(key)
    except ValueError:
        # The counter expired between add and incr: this is the first hit
        # of a fresh window.
        cache.add(key, 1, timeout=int(HANDOFF_RATE_WINDOW.total_seconds()) + 1)
        return False
    return count > HANDOFF_RATE_LIMIT


#: Dead codes (expired or consumed) are deleted on the next issue once they
#: are this old. Far past the 60-second TTL and the rate window (the limiter
#: counts in the cache, not these rows), so nothing live is touched.
HANDOFF_RETENTION = timedelta(hours=1)


def _prune_dead_codes(user, now) -> None:
    """Delete ``user``'s expired or consumed codes older than the retention."""
    cutoff = now - HANDOFF_RETENTION
    DesktopHandoffCode.objects.filter(user=user, created_at__lt=cutoff).filter(
        Q(consumed_at__isnull=False) | Q(expires_at__lte=now)
    ).delete()


def _challenge_matches(verifier: bytes, stored: str) -> bool:
    """Constant-time compare of SHA-256(verifier) with the stored challenge."""
    return hmac.compare_digest(
        expected_challenge_for(verifier).encode("utf-8"),
        (stored or "").encode("utf-8"),
    )


class HandoffError(Exception):
    """A handoff refusal; ``code`` is the wire code, ``http_status`` the reply."""

    def __init__(self, code: str, http_status: int):
        super().__init__(code)
        self.code = code
        self.http_status = http_status


def issue_handoff(*, user, dev_machine, challenge: str, machine_token) -> DesktopHandoffCode:
    """Store a single-use code for ``dev_machine`` bound to ``user``.

    ``machine_token`` is the daemon credential that asked; the code is
    refused on redeem once that credential is revoked or expired (§5.1).
    Dead codes of ``user`` past the retention are pruned here.

    Raises :class:`HandoffError` (``handoff_disabled``, ``rate_limited``).
    The raw code is returned on the row as ``_raw`` (never stored).
    """
    now = timezone.now()
    if not handoff_enabled():
        logger.info(
            "desktop_handoff.issue_refused user=%s dev_machine=%s reason=%s",
            user.id,
            dev_machine.id,
            HANDOFF_DISABLED,
        )
        raise HandoffError(HANDOFF_DISABLED, 410)
    if _rate_limited(dev_machine.id, now):
        logger.info(
            "desktop_handoff.issue_refused user=%s dev_machine=%s reason=rate_limited",
            user.id,
            dev_machine.id,
        )
        raise HandoffError("rate_limited", 429)
    raw = "dh_" + secrets.token_urlsafe(32)
    with transaction.atomic():
        _prune_dead_codes(user, now)
        row = DesktopHandoffCode.objects.create(
            code_hash=hash_token(raw),
            fingerprint=fingerprint(raw),
            user=user,
            dev_machine=dev_machine,
            machine_token=machine_token,
            challenge=challenge,
            expires_at=now + HANDOFF_TTL,
        )
    row._raw = raw  # noqa: SLF001 — handed once to the daemon, never stored again.
    logger.info(
        "desktop_handoff.issued user=%s dev_machine=%s fingerprint=%s",
        user.id,
        dev_machine.id,
        row.fingerprint,
    )
    return row


def redeem_handoff(*, code: str, verifier: bytes, expected_user_id: str) -> dict:
    """Consume ``code`` and, when every check passes, create the session.

    The four checks (design §5 step 6): the code is live and unused;
    SHA-256(verifier) matches the stored challenge; the code's user is
    ``expected_user_id``; the user and the machine are active. The code is
    consumed on the first redeem attempt whatever the outcome; only then is
    the session created, so a leaked code cannot be replayed. Returns the
    session payload ``{"user_id", "email"}``. The caller creates the web
    session; this function decides and consumes.

    Raises :class:`HandoffError` with a stable wire code.
    """
    now = timezone.now()
    # The consume commits on its own: the first redeem attempt uses the code
    # up, pass or fail, so a leaked code is single-use (design §5 step 6).
    # select_for_update + a verdict (not a raise) keeps that commit even on
    # mismatch — raising inside the atomic block would roll the consume back.
    verdict = _consume_and_check(code, verifier, expected_user_id, now)
    if isinstance(verdict, HandoffError):
        raise verdict
    user, machine, _row = verdict
    logger.info(
        "desktop_handoff.redeemed user=%s dev_machine=%s fingerprint=%s",
        user.id,
        machine.id,
        _row.fingerprint,
    )
    return {"user_id": str(user.id), "email": user.email, "user": user, "dev_machine": machine}


def _consume_and_check(code: str, verifier: bytes, expected_user_id: str, now):
    """Lock, consume and check ``code`` in one transaction.

    Returns ``(user, machine, row)`` on success, else a :class:`HandoffError`
    describing the refusal (the code stays consumed either way, except when
    it was already dead or the caller was rate-limited before consuming).
    """
    with transaction.atomic():
        try:
            row = DesktopHandoffCode.objects.select_for_update().get(code_hash=hash_token(code))
        except DesktopHandoffCode.DoesNotExist:
            logger.info("desktop_handoff.redeem_refused reason=unknown_code")
            return HandoffError("invalid_code", 400)
        if row.consumed_at is not None:
            logger.info(
                "desktop_handoff.redeem_refused user=%s dev_machine=%s reason=already_consumed",
                row.user_id,
                row.dev_machine_id,
            )
            return HandoffError("invalid_code", 400)
        if row.is_expired(now):
            logger.info(
                "desktop_handoff.redeem_refused user=%s dev_machine=%s reason=expired",
                row.user_id,
                row.dev_machine_id,
            )
            return HandoffError("expired_code", 410)
        machine_id = row.dev_machine_id
        if _rate_limited(f"{machine_id}:redeem", now):
            logger.info(
                "desktop_handoff.redeem_refused user=%s dev_machine=%s reason=rate_limited",
                row.user_id,
                machine_id,
            )
            return HandoffError("rate_limited", 429)
        row.consumed_at = now
        row.save(update_fields=["consumed_at"])
        user = row.user
        machine = row.dev_machine
        if not _challenge_matches(verifier, row.challenge):
            logger.info(
                "desktop_handoff.redeem_refused user=%s dev_machine=%s reason=verifier_mismatch",
                row.user_id,
                machine_id,
            )
            return HandoffError("invalid_grant", 400)
        if str(user.id) != str(expected_user_id):
            logger.info(
                "desktop_handoff.redeem_refused user=%s dev_machine=%s reason=user_mismatch",
                row.user_id,
                machine_id,
            )
            return HandoffError("invalid_grant", 400)
        if not user.is_active:
            logger.info(
                "desktop_handoff.redeem_refused user=%s dev_machine=%s reason=user_inactive",
                row.user_id,
                machine_id,
            )
            return HandoffError("invalid_grant", 400)
        if machine is None or machine.revoked_at is not None or machine.owner_id != user.id:
            logger.info(
                "desktop_handoff.redeem_refused user=%s dev_machine=%s reason=machine_revoked",
                row.user_id,
                machine_id,
            )
            return HandoffError("invalid_grant", 400)
        # The daemon credential the code was issued with must still be live
        # (design §5.1): same refusal as a revoked machine.
        token = row.machine_token
        if token is None or token.revoked_at is not None or token.is_expired(now):
            logger.info(
                "desktop_handoff.redeem_refused user=%s dev_machine=%s reason=machine_token_revoked",
                row.user_id,
                machine_id,
            )
            return HandoffError("invalid_grant", 400)
        return user, machine, row
