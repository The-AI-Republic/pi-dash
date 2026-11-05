# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Automatic engine pause reasons (built_engine_as_runner §8, §9).

``DevMachine.engine_auto_pause_reason`` is set by the server from three
sources and cleared when they recover:

- the daemon's engine report in the machine-session heartbeat
  (:func:`record_engine_state_report`);
- the model-token endpoint's own outcome (:func:`apply_model_token_outcome`);
- the profile endpoints (:func:`apply_profile_outcome`), for new-path
  machines and, through the desktop-session endpoints, legacy ones.

Each source clears only the reasons it can disprove, so one source never
undoes a pause another set for a reason it cannot see. On legacy desktop
machines ``engine_sign_in_required`` never applies (§9): they have no engine
credential.
"""

from __future__ import annotations

import logging

from django.db import transaction
from django.utils import timezone

from pi_dash.runner.models import DevMachine, EngineAutoPauseReason, RunnerProvisioning
from pi_dash.runner.services.dispatch_rules import set_engine_auto_pause_reason

logger = logging.getLogger(__name__)

R = EngineAutoPauseReason

#: Reasons the daemon's own report may set (a daemon-local code such as
#: ``engine_not_installed`` or ``profile_pending`` is stored, not applied).
_REPORTABLE = frozenset({R.ENGINE_SIGN_IN_REQUIRED, R.GATEWAY_SESSION_REVOKED, R.WALLET_EMPTY})
#: Reasons a daemon reporting a working engine disproves. The instance-level
#: ones (``profile_unavailable``, ``engine_disabled``) are the profile
#: endpoint's to clear; the daemon re-reads the profile every 15 minutes.
_CLEARED_BY_HEALTHY_REPORT = frozenset({R.GATEWAY_SESSION_REVOKED, R.WALLET_EMPTY, R.ENGINE_SIGN_IN_REQUIRED})
#: Reasons a successfully issued model token disproves. ``wallet_empty`` is
#: not one: minting a token does not touch the wallet, so only the daemon
#: (which sees the gateway's 402) can say it recovered.
_CLEARED_BY_MODEL_TOKEN = frozenset(
    {R.ENGINE_SIGN_IN_REQUIRED, R.GATEWAY_SESSION_REVOKED, R.PROFILE_UNAVAILABLE, R.ENGINE_DISABLED}
)
_CLEARED_BY_PROFILE = frozenset({R.PROFILE_UNAVAILABLE, R.ENGINE_DISABLED})

_HEALTHY_STATES = frozenset({"available", "expiring"})
#: Pause reasons a daemon report never replaces: the profile endpoint owns them.
_NEVER_REPLACED_BY_A_REPORT = (R.ENGINE_DISABLED,)
_SIGNED_OUT_STATES = frozenset({"expired", "signed_out", "revoked"})
_MAX_FIELD = 64


def _apply(machine: DevMachine, reason: str, *, keep=()) -> bool:
    if reason == R.ENGINE_SIGN_IN_REQUIRED and machine.is_legacy_desktop:
        return False
    if machine.revoked_at is not None:
        return False
    return set_engine_auto_pause_reason(machine, reason, keep=keep)


def _clear(machine: DevMachine, clearable) -> bool:
    current = machine.engine_auto_pause_reason
    if not current or current not in clearable:
        return False
    if current == R.ENGINE_SIGN_IN_REQUIRED and not machine.is_legacy_desktop:
        # The server's own view wins: a daemon cannot clear a sign-in pause
        # while the server holds no live credential for the machine.
        if not machine.has_live_engine_credential():
            return False
    return set_engine_auto_pause_reason(machine, R.NONE, only_from=clearable)


def _clean(value) -> str:
    if value is None:
        return ""
    return str(value).strip()[:_MAX_FIELD]


def record_engine_state_report(machine: DevMachine, report: dict) -> None:
    """Store the heartbeat's ``engine_state`` and derive the automatic pause.

    ``report`` is ``{"state", "reason_code"?, "sign_in_state"?,
    "credential_expires_at"?}``; ``state`` is one of ``available``,
    ``signed_out``, ``expiring``, ``unavailable``.
    """
    state = _clean(report.get("state"))
    reason_code = _clean(report.get("reason_code"))
    stored = {"state": state, "reason_code": reason_code}
    for key in ("sign_in_state", "credential_expires_at"):
        if report.get(key) not in (None, ""):
            stored[key] = _clean(report.get(key))
    DevMachine.objects.filter(pk=machine.pk).update(engine_state=stored, engine_state_reported_at=timezone.now())
    machine.engine_state = stored

    fresh = DevMachine.objects.filter(pk=machine.pk).first()
    if fresh is None:
        return
    if fresh.engine_auto_pause_reason == R.ENGINE_DISABLED:
        # The instance switched the engine off. That is the profile
        # endpoint's to set and clear; a daemon report never replaces it,
        # or the machine page would ask for a sign-in that cannot help.
        pass
    # ``keep``: re-checked under the row lock, since the instance may switch
    # the engine off between the read above and the write.
    elif reason_code in _REPORTABLE:
        _apply(fresh, reason_code, keep=_NEVER_REPLACED_BY_A_REPORT)
    elif state == "signed_out" or stored.get("sign_in_state") in _SIGNED_OUT_STATES:
        _apply(fresh, R.ENGINE_SIGN_IN_REQUIRED, keep=_NEVER_REPLACED_BY_A_REPORT)
    elif state in _HEALTHY_STATES and not reason_code:
        _clear(fresh, _CLEARED_BY_HEALTHY_REPORT)
    machine.engine_auto_pause_reason = fresh.engine_auto_pause_reason


def pause_engines_with_expired_credentials() -> int:
    """Pause the engine of every new-path machine whose engine credential ran
    out, with ``engine_sign_in_required`` (§4.3 "Expiry"), so pods route
    elsewhere even while the daemon is offline. Returns the number paused."""
    expired = DevMachine.objects.filter(
        engine_sign_in_state="active",
        engine_credential_expires_at__lte=timezone.now(),
        engine_auto_pause_reason="",
        revoked_at__isnull=True,
    ).exclude(provisioning=RunnerProvisioning.DESKTOP_BUNDLED)
    paused = 0
    for machine_id in expired.values_list("pk", flat=True):
        with transaction.atomic():
            # Re-checked on the locked row: a renewal that committed after
            # the query above must not be paused.
            machine = (
                DevMachine.objects.select_for_update()
                .filter(
                    pk=machine_id,
                    engine_sign_in_state="active",
                    engine_credential_expires_at__lte=timezone.now(),
                    engine_auto_pause_reason="",
                    revoked_at__isnull=True,
                )
                .first()
            )
            if machine is not None and _apply(machine, R.ENGINE_SIGN_IN_REQUIRED):
                paused += 1
    return paused


def apply_model_token_outcome(machine: DevMachine, reason: str) -> None:
    """Map a model-token outcome onto the machine (``""`` = success)."""
    if reason:
        _apply(machine, reason)
    else:
        _clear(machine, _CLEARED_BY_MODEL_TOKEN)


def apply_profile_outcome(machine: DevMachine, reason: str) -> None:
    """Map a profile read onto the machine: ``engine_disabled`` /
    ``profile_unavailable``, or ``""`` when the profile is available."""
    if reason:
        # A sharper reason already set (sign-in, wallet, gateway) stays: the
        # engine is paused either way, and that one says what to fix.
        if not machine.engine_auto_pause_reason or machine.engine_auto_pause_reason in _CLEARED_BY_PROFILE:
            _apply(machine, reason)
    else:
        _clear(machine, _CLEARED_BY_PROFILE)


def apply_profile_outcome_to_legacy_machines(user, reason: str) -> None:
    """The desktop-session endpoints' share of §9: a legacy desktop machine
    takes ``wallet_empty`` / ``profile_unavailable`` / ``engine_disabled``
    from those endpoints' failures. The endpoints are per user, not per
    machine, so the outcome applies to all of the user's live legacy machines."""
    machines = DevMachine.objects.filter(
        owner=user,
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        revoked_at__isnull=True,
    )
    for machine in machines:
        try:
            if reason in (R.PROFILE_UNAVAILABLE, R.ENGINE_DISABLED, ""):
                apply_profile_outcome(machine, reason)
            elif reason == R.WALLET_EMPTY:
                _apply(machine, reason)
        except Exception:  # pragma: no cover - never fail the desktop request
            logger.exception("legacy engine pause update failed for machine %s", machine.id)
