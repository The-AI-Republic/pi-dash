# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Engine credentials: issue, authenticate, revoke (built_engine_as_runner §4.3).

An engine credential (``ec_…``) belongs to one dev machine and does exactly
one thing: obtain a short-lived model token for the built-in engine. It is
issued with the daemon credential by ``pidash auth login``, or alone through
an ``engine``-scoped device grant redeemed with the machine token.

This module is the only writer of ``EngineCredential`` rows and of
``DevMachine.engine_sign_in_state`` / ``engine_credential_expires_at``; each
change to one is made in the same transaction as the other. Every issue, use
failure and revocation is logged with the user and machine, never the raw
value.
"""

from __future__ import annotations

import logging
import secrets
from dataclasses import dataclass
from typing import Optional

from django.db import transaction
from django.utils import timezone

from pi_dash.runner.models import (
    ENGINE_CREDENTIAL_TTL,
    DevMachine,
    EngineAutoPauseReason,
    EngineCredential,
    EngineCredentialRevokeReason,
    EngineSignInState,
)
from pi_dash.runner.services.tokens import fingerprint, hash_token

logger = logging.getLogger("pi_dash.runner.engine_credential")

ENGINE_CREDENTIAL_PREFIX = "ec_"

#: The only sign-in states in which an engine credential may be issued on a
#: silent (non-interactive) approval, e.g. the desktop app's renewal (design
#: §4.3 table). An allow-list on purpose: ``never`` needs an explicit "Turn
#: on the built-in engine", ``active`` is not yet due for renewal (renewing it
#: silently would extend the fixed 14-day lifetime forever), and
#: ``signed_out`` / ``revoked`` are cleared only by an explicit sign-in.
SILENT_RENEWAL_ALLOWED_STATES = frozenset({EngineSignInState.EXPIRING, EngineSignInState.EXPIRED})

SILENT_RENEWAL_REFUSED = "engine_silent_renewal_refused"


def silent_renewal_allowed(state: str) -> bool:
    """True when a non-interactive approval may (re)issue the engine credential."""
    return state in SILENT_RENEWAL_ALLOWED_STATES

#: Revocation reason → stored machine sign-in state.
_STATE_FOR_REASON = {
    EngineCredentialRevokeReason.SIGNED_OUT: EngineSignInState.SIGNED_OUT,
    EngineCredentialRevokeReason.USER_SIGNED_OUT: EngineSignInState.SIGNED_OUT,
    EngineCredentialRevokeReason.MACHINE_REMOVED: EngineSignInState.REVOKED,
    EngineCredentialRevokeReason.MACHINE_MERGED: EngineSignInState.REVOKED,
    EngineCredentialRevokeReason.ADMIN: EngineSignInState.REVOKED,
}


@dataclass(frozen=True)
class IssuedEngineCredential:
    raw: str
    credential: EngineCredential

    @property
    def expires_at(self):
        return self.credential.expires_at

    def as_payload(self) -> dict:
        """The ``engine_credential`` object of the device-flow responses."""
        return {"token": self.raw, "expires_at": self.credential.expires_at.isoformat()}


class EngineCredentialError(Exception):
    """An engine credential that cannot be used; ``code`` is the wire code."""

    def __init__(self, code: str, credential: Optional[EngineCredential] = None):
        super().__init__(code)
        self.code = code
        self.credential = credential


def engine_profile_for_user(user):
    """``(available, profile, pause_reason)`` for serving the engine to ``user``.

    ``pause_reason`` is the automatic pause reason the state maps to:
    ``engine_disabled`` when the instance kill switch is off,
    ``profile_unavailable`` when the model profile is, else empty.
    """
    from pi_dash.core.agent_execution import managed_runner_is_enabled
    from pi_dash.ee.assistant.model_provider import agent_model_profile_for_user

    profile = agent_model_profile_for_user(user)
    if not managed_runner_is_enabled():
        return False, profile, EngineAutoPauseReason.ENGINE_DISABLED
    if not profile.available:
        return False, profile, EngineAutoPauseReason.PROFILE_UNAVAILABLE
    return True, profile, EngineAutoPauseReason.NONE


def instance_can_serve_engine(user) -> bool:
    """True when ``pidash auth login`` should issue an engine credential."""
    available, _profile, _reason = engine_profile_for_user(user)
    return available


def _mint_raw() -> str:
    return ENGINE_CREDENTIAL_PREFIX + secrets.token_urlsafe(32)


def issue_engine_credential(*, user, dev_machine: DevMachine, silent: bool = False) -> IssuedEngineCredential:
    """Issue a new engine credential for ``dev_machine``, revoking the live one.

    Must run inside the caller's transaction when it is part of a larger
    exchange (the device flow issues the machine token in the same one).
    Clears an ``engine_sign_in_required`` automatic pause on a new-path
    machine: the engine has a valid credential again.

    ``silent``: the issue comes from a non-interactive approval. The sign-in
    state is then re-checked on the locked row, so a sign-out that committed
    after the caller's unlocked check is never overwritten; raises
    :class:`EngineCredentialError` (``engine_silent_renewal_refused``).
    """
    if dev_machine.owner_id != user.id:
        raise ValueError("engine credentials are issued only to the machine's owner")
    from pi_dash.runner.services.dispatch_rules import set_engine_auto_pause_reason

    now = timezone.now()
    with transaction.atomic():
        machine = DevMachine.objects.select_for_update().get(pk=dev_machine.pk)
        if silent and not silent_renewal_allowed(machine.current_engine_sign_in_state(now)):
            raise EngineCredentialError(SILENT_RENEWAL_REFUSED)
        superseded = EngineCredential.objects.filter(dev_machine=machine, revoked_at__isnull=True).update(
            revoked_at=now, revoked_reason=EngineCredentialRevokeReason.SUPERSEDED
        )
        raw = _mint_raw()
        credential = EngineCredential.objects.create(
            user=user,
            dev_machine=machine,
            token_hash=hash_token(raw),
            fingerprint=fingerprint(raw),
            expires_at=now + ENGINE_CREDENTIAL_TTL,
        )
        machine.engine_sign_in_state = EngineSignInState.ACTIVE
        machine.engine_credential_expires_at = credential.expires_at
        machine.save(update_fields=["engine_sign_in_state", "engine_credential_expires_at", "updated_at"])
        dev_machine.engine_sign_in_state = machine.engine_sign_in_state
        dev_machine.engine_credential_expires_at = machine.engine_credential_expires_at
        if machine.engine_auto_pause_reason == EngineAutoPauseReason.ENGINE_SIGN_IN_REQUIRED:
            set_engine_auto_pause_reason(machine, EngineAutoPauseReason.NONE)
            dev_machine.engine_auto_pause_reason = EngineAutoPauseReason.NONE
    logger.info(
        "engine_credential.issued user=%s dev_machine=%s fingerprint=%s expires_at=%s superseded=%s",
        user.id,
        machine.id,
        credential.fingerprint,
        credential.expires_at.isoformat(),
        superseded,
    )
    return IssuedEngineCredential(raw=raw, credential=credential)


def revoke_engine_credential(dev_machine: DevMachine, *, reason: str) -> bool:
    """Revoke ``dev_machine``'s live engine credential and record the state.

    The machine's state follows the reason (``signed_out`` or ``revoked``)
    even when no credential is live, so an explicit sign-out of an expired
    engine still blocks the app's silent renewal. On a new-path machine the
    engine is paused automatically with ``engine_sign_in_required``.
    Returns True when a live credential was revoked.
    """
    from pi_dash.runner.services.dispatch_rules import set_engine_auto_pause_reason

    state = _STATE_FOR_REASON.get(reason)
    if state is None:
        raise ValueError(f"unknown engine credential revoke reason {reason!r}")
    now = timezone.now()
    with transaction.atomic():
        machine = DevMachine.objects.select_for_update().filter(pk=dev_machine.pk).first()
        if machine is None:
            return False
        revoked = EngineCredential.objects.filter(dev_machine=machine, revoked_at__isnull=True).update(
            revoked_at=now, revoked_reason=reason
        )
        had_engine = machine.engine_sign_in_state != EngineSignInState.NEVER or bool(revoked)
        machine.engine_sign_in_state = state
        machine.engine_credential_expires_at = None
        machine.save(update_fields=["engine_sign_in_state", "engine_credential_expires_at", "updated_at"])
        dev_machine.engine_sign_in_state = state
        dev_machine.engine_credential_expires_at = None
        if (
            had_engine
            and not machine.is_legacy_desktop
            and machine.revoked_at is None
            and not machine.engine_auto_pause_reason
        ):
            set_engine_auto_pause_reason(machine, EngineAutoPauseReason.ENGINE_SIGN_IN_REQUIRED)
            dev_machine.engine_auto_pause_reason = EngineAutoPauseReason.ENGINE_SIGN_IN_REQUIRED
    logger.info(
        "engine_credential.revoked dev_machine=%s user=%s reason=%s revoked=%s",
        machine.id,
        machine.owner_id,
        reason,
        revoked,
    )
    return bool(revoked)


def revoke_engine_credentials_for_user(user, *, reason: str = EngineCredentialRevokeReason.USER_SIGNED_OUT) -> int:
    """Revoke every live engine credential of ``user`` (sign-out everywhere)."""
    machine_ids = list(
        EngineCredential.objects.filter(user=user, revoked_at__isnull=True)
        .values_list("dev_machine_id", flat=True)
        .distinct()
    )
    count = 0
    for machine in DevMachine.objects.filter(pk__in=machine_ids):
        if revoke_engine_credential(machine, reason=reason):
            count += 1
    return count


def authenticate_engine_credential(raw: str) -> EngineCredential:
    """Resolve a presented ``ec_`` value to its live credential.

    Raises :class:`EngineCredentialError` with ``engine_sign_in_required``
    for an unknown, expired or revoked credential (the daemon's fix is the
    same: sign the engine in again), or ``dev_machine_revoked`` when its
    machine is gone. Bumps ``last_used_at``; never extends ``expires_at``.
    """
    if not raw or not raw.startswith(ENGINE_CREDENTIAL_PREFIX):
        raise EngineCredentialError("engine_sign_in_required")
    credential = (
        EngineCredential.objects.select_related("user", "dev_machine").filter(token_hash=hash_token(raw)).first()
    )
    if credential is None:
        raise EngineCredentialError("engine_sign_in_required")
    if credential.revoked_at is not None or credential.is_expired():
        raise EngineCredentialError("engine_sign_in_required", credential)
    machine = credential.dev_machine
    if machine.revoked_at is not None or machine.owner_id != credential.user_id:
        raise EngineCredentialError("dev_machine_revoked", credential)
    if not credential.user.is_active:
        raise EngineCredentialError("engine_sign_in_required", credential)
    EngineCredential.objects.filter(pk=credential.pk).update(last_used_at=timezone.now())
    return credential


def engine_credential_payload(machine: DevMachine) -> dict:
    """Engine fields shared by the machine payloads and the device flow."""
    return {
        "engine_sign_in_state": machine.current_engine_sign_in_state(),
        "engine_credential_expires_at": (
            machine.engine_credential_expires_at.isoformat() if machine.engine_credential_expires_at else None
        ),
    }
