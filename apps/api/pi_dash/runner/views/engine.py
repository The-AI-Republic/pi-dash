# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Built-in engine endpoints for the CLI daemon (built_engine_as_runner §4.3).

- ``GET  /api/v1/runner/engine/profile/`` — daemon credential (``mt_``). The
  model endpoint the engine should call, and whether it may. No secret.
- ``POST /api/v1/runner/engine/model-token/`` — engine credential (``ec_``)
  only. A short-lived model token; rate-limited and audited.
- ``POST /api/v1/runner/engine/credential/revoke/`` — daemon credential.
  Signs the machine's engine out (``pidash auth logout --engine``).

Both profile and model-token outcomes feed the machine's automatic engine
pause (§8) through :mod:`pi_dash.runner.services.engine_state`.
"""

from __future__ import annotations

import hashlib
import logging
import uuid as _uuid

from django.db import transaction
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response
from rest_framework.throttling import SimpleRateThrottle
from rest_framework.views import APIView

from pi_dash.runner.authentication import MachineTokenAuthentication
from pi_dash.runner.models import DevMachine, EngineAutoPauseReason, EngineCredential, EngineCredentialRevokeReason
from pi_dash.runner.services.engine_credentials import (
    EngineCredentialError,
    authenticate_engine_credential,
    engine_credential_payload,
    engine_profile_for_user,
    revoke_engine_credential,
)
from pi_dash.runner.services.engine_state import apply_model_token_outcome, apply_profile_outcome

logger = logging.getLogger("pi_dash.runner.engine")

R = EngineAutoPauseReason


def _token_machine(request):
    token = getattr(request, "auth_machine_token", None)
    if token is None or token.dev_machine_id is None:
        return None
    return token.dev_machine


def _machine_required():
    return Response(
        {"error": "dev_machine_required", "detail": "This machine token is not bound to a computer."},
        status=status.HTTP_403_FORBIDDEN,
    )


def _bearer(request) -> str:
    parts = (request.headers.get("Authorization") or "").split()
    if len(parts) == 2 and parts[0].lower() == "bearer":
        return parts[1]
    return ""


def _request_id(request) -> str:
    supplied = (request.headers.get("X-Request-Id") or "").strip()[:64]
    return supplied or _uuid.uuid4().hex


class EngineProfileEndpoint(APIView):
    """``GET`` the engine's model profile for this machine's owner.

    ``available`` folds in the instance kill switch. ``reason_code`` is
    ``engine_disabled`` when the switch is off, else the profile's own code
    (for example ``llm_config_missing``); ``pause_reason`` is the automatic
    pause reason the answer maps to.
    """

    authentication_classes = [MachineTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def get(self, request):
        machine = _token_machine(request)
        if machine is None:
            return _machine_required()
        available, profile, pause_reason = engine_profile_for_user(machine.owner)
        if not machine.is_legacy_desktop:
            apply_profile_outcome(machine, pause_reason)
        if pause_reason == R.ENGINE_DISABLED:
            reason_code = R.ENGINE_DISABLED
        else:
            reason_code = "" if available else (profile.reason_code or R.PROFILE_UNAVAILABLE)
        return Response(
            {
                "available": available,
                "lane": profile.lane,
                "base_url": profile.base_url,
                "model": profile.model,
                "reason_code": reason_code,
                "pause_reason": pause_reason,
                "top_up_url": getattr(profile, "top_up_url", ""),
                **engine_credential_payload(machine),
            }
        )


class EngineModelTokenThrottle(SimpleRateThrottle):
    """Per engine credential (falls back to the client address)."""

    scope = "engine_model_token"

    def get_cache_key(self, request, view):
        raw = _bearer(request)
        ident = hashlib.sha256(raw.encode()).hexdigest()[:32] if raw else self.get_ident(request)
        return self.cache_format % {"scope": self.scope, "ident": ident}


def _classify_gateway_error(exc: Exception) -> str:
    """Map a credential failure onto a pause reason, or ``""`` if transient."""
    name = type(exc).__name__
    if name in ("OpenHubAuthError", "SessionRevoked", "NoAIRepublicSession"):
        return R.GATEWAY_SESSION_REVOKED
    lowered = name.lower()
    if "wallet" in lowered or "insufficient" in lowered or "paymentrequired" in lowered:
        return R.WALLET_EMPTY
    return ""


_STATUS_FOR_REASON = {
    R.ENGINE_SIGN_IN_REQUIRED: status.HTTP_401_UNAUTHORIZED,
    R.GATEWAY_SESSION_REVOKED: status.HTTP_409_CONFLICT,
    R.WALLET_EMPTY: status.HTTP_402_PAYMENT_REQUIRED,
    R.PROFILE_UNAVAILABLE: status.HTTP_409_CONFLICT,
    R.ENGINE_DISABLED: status.HTTP_409_CONFLICT,
}


class EngineModelTokenEndpoint(APIView):
    """``POST`` a short-lived model token, authenticated by the engine
    credential only (``Authorization: Bearer ec_…``).

    Answers ``{token, expires_at}``. Failures answer ``{reason_code,
    detail}`` with ``reason_code`` one of ``engine_sign_in_required`` (401),
    ``gateway_session_revoked`` (409), ``wallet_empty`` (402),
    ``profile_unavailable`` (409) or ``engine_disabled`` (409), and set the
    machine's automatic pause to it; success clears what it disproves. A
    transient gateway failure is a 503 without a pause.
    """

    authentication_classes: list = []
    permission_classes: list = []
    throttle_classes = [EngineModelTokenThrottle]

    def post(self, request):
        from pi_dash.ee.assistant.model_provider import agent_model_credential_for_user
        from pi_dash.managed_runner.errors import ManagedRunnerUnavailable

        request_id = _request_id(request)
        try:
            credential = authenticate_engine_credential(_bearer(request))
        except EngineCredentialError as exc:
            if exc.credential is not None and exc.code == R.ENGINE_SIGN_IN_REQUIRED:
                # An old request may arrive after a renewal. Reject its token
                # without pausing the replacement credential's healthy engine.
                # Serialize this check with credential issuance/revocation.
                with transaction.atomic():
                    machine = DevMachine.objects.select_for_update().get(pk=exc.credential.dev_machine_id)
                    if not EngineCredential.objects.filter(
                        dev_machine=machine, revoked_at__isnull=True, expires_at__gt=timezone.now()
                    ).exists():
                        apply_model_token_outcome(machine, R.ENGINE_SIGN_IN_REQUIRED)
            logger.info(
                "engine.model_token_refused request_id=%s user=%s dev_machine=%s reason=%s",
                request_id,
                getattr(exc.credential, "user_id", None),
                getattr(exc.credential, "dev_machine_id", None),
                exc.code,
            )
            return Response(
                {
                    "reason_code": R.ENGINE_SIGN_IN_REQUIRED,
                    "error": exc.code,
                    "detail": "Sign the built-in engine in again (`pidash login --engine`).",
                },
                status=status.HTTP_401_UNAUTHORIZED,
            )

        user, machine = credential.user, credential.dev_machine
        available, profile, pause_reason = engine_profile_for_user(user)
        reason, detail = "", ""
        token = expires_at = None
        if not available:
            reason = pause_reason
            detail = profile.reason_code if pause_reason == R.PROFILE_UNAVAILABLE else "The engine is turned off."
        else:
            try:
                token, expires_at = agent_model_credential_for_user(user)
            except ManagedRunnerUnavailable as exc:
                reason, detail = R.PROFILE_UNAVAILABLE, exc.code
            except Exception as exc:  # noqa: BLE001 — classified, never echoed raw
                reason = _classify_gateway_error(exc)
                if not reason:
                    logger.warning(
                        "engine.model_token_transient request_id=%s user=%s dev_machine=%s error=%s",
                        request_id,
                        user.id,
                        machine.id,
                        type(exc).__name__,
                    )
                    return Response(
                        {"error": "gateway_unavailable", "detail": "Try again shortly."},
                        status=status.HTTP_503_SERVICE_UNAVAILABLE,
                    )
                detail = type(exc).__name__

        apply_model_token_outcome(machine, reason)
        if reason:
            logger.info(
                "engine.model_token_refused request_id=%s user=%s dev_machine=%s reason=%s",
                request_id,
                user.id,
                machine.id,
                reason,
            )
            return Response({"reason_code": reason, "detail": detail}, status=_STATUS_FOR_REASON[reason])

        expires_iso = getattr(expires_at, "isoformat", lambda: expires_at)()
        # Audit the issuance, never the token.
        logger.info(
            "engine.model_token_issued request_id=%s user=%s dev_machine=%s credential=%s expires_at=%s",
            request_id,
            user.id,
            machine.id,
            credential.fingerprint,
            expires_iso,
        )
        return Response({"token": token, "expires_at": expires_iso})


class EngineCredentialRevokeEndpoint(APIView):
    """``POST`` sign this machine's engine out (state ``signed_out``).

    Idempotent. Afterwards the desktop app's silent renewal is refused until
    the engine is signed in explicitly.
    """

    authentication_classes = [MachineTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def post(self, request):
        machine = _token_machine(request)
        if machine is None:
            return _machine_required()
        revoked = revoke_engine_credential(machine, reason=EngineCredentialRevokeReason.SIGNED_OUT)
        return Response({"ok": True, "revoked": revoked, **engine_credential_payload(machine)})
