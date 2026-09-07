# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Model profile and credential for the desktop-bundled agent engine.

The desktop app does not read ``users/me/ai-assistant/config/`` and
re-implement provider resolution — it asks these two endpoints, which run the
same ``resolve_model_for_user`` seam Pi Dash AI and the Cloud Agent use. That
is what keeps "which provider am I on" from drifting between surfaces.

The split is deliberate: the profile is safe to poll and carries no secret, so
the app can refresh it on window focus; the credential is minted on demand,
rate-limited and audited.
"""

from __future__ import annotations

import logging

from django.conf import settings
from rest_framework import status
from rest_framework.response import Response
from rest_framework.throttling import UserRateThrottle

from pi_dash.app.views.base import BaseAPIView
from pi_dash.core.agent_execution import managed_runner_is_enabled
from pi_dash.managed_runner.errors import ManagedRunnerUnavailable
from pi_dash.managed_runner.permissions import IsDesktopSession

logger = logging.getLogger(__name__)


class AgentModelProfileEndpoint(BaseAPIView):
    """``GET`` the endpoint the bundled engine should call, and whether it may.

    Never returns a credential. ``managed_runner_enabled`` travels here rather
    than being baked into the desktop bundle so the operator kill switch takes
    effect without shipping a new app build.
    """

    permission_classes = [IsDesktopSession]

    def get(self, request):
        from pi_dash.ee.assistant.model_provider import agent_model_profile_for_user

        profile = agent_model_profile_for_user(request.user)
        enabled = managed_runner_is_enabled()
        return Response(
            {
                "managed_runner_enabled": enabled,
                "lane": profile.lane,
                "base_url": profile.base_url,
                "model": profile.model,
                # The instance switch is part of "can I run here", so fold it in
                # rather than making the client combine two flags and risk
                # showing an available engine on a disabled instance.
                "available": bool(enabled and profile.available),
                "reason_code": "" if (enabled and profile.available) else (
                    profile.reason_code or "managed_runner_disabled"
                ),
                "graceful_stop_seconds": settings.MANAGED_RUNNER_GRACEFUL_STOP_SECS,
            }
        )


class AgentModelTokenThrottle(UserRateThrottle):
    scope = "assistant_agent_token"


class AgentModelTokenEndpoint(BaseAPIView):
    """``POST`` a short-lived credential for the bundled engine.

    The desktop writes the returned token to a ``0600`` file the daemon reads
    once per agent spawn, and re-fetches before expiry. Nothing is persisted
    server-side by this call; it is a read of the user's existing session
    credential, which is why it is throttled and audited by user rather than
    treated as a mutation.
    """

    permission_classes = [IsDesktopSession]
    throttle_classes = [AgentModelTokenThrottle]

    def post(self, request):
        from pi_dash.ee.assistant.model_provider import agent_model_credential_for_user

        try:
            token, expires_at = agent_model_credential_for_user(request.user)
        except ManagedRunnerUnavailable as exc:
            # The client should have consulted the profile first; answer with
            # the same reason code it would have seen there.
            return Response(
                {"error": exc.code, "detail": str(exc)},
                status=status.HTTP_409_CONFLICT,
            )
        except Exception as exc:  # noqa: BLE001 — classified below, never echoed raw
            code, http_status = _classify_credential_error(exc)
            logger.warning("managed_runner.token_refresh_failed user=%s code=%s", request.user.id, code)
            return Response({"error": code}, status=http_status)

        # Audit the issuance, never the token.
        logger.info(
            "managed_runner.token_issued user=%s expires_at=%s",
            request.user.id,
            getattr(expires_at, "isoformat", lambda: expires_at)(),
        )
        return Response(
            {
                "token": token,
                "expires_at": getattr(expires_at, "isoformat", lambda: expires_at)(),
            }
        )


def _classify_credential_error(exc: Exception) -> tuple[str, int]:
    """Map a credential failure onto a stable code the desktop can act on.

    ``401`` means "sign in again" (the app stops the daemon and prompts);
    ``503`` means "retry later" (the app keeps the current token until it
    expires). Anything unrecognised is treated as transient rather than
    signing the user out on a bug.
    """
    name = type(exc).__name__
    if name in ("OpenHubAuthError", "SessionRevoked", "NoAIRepublicSession"):
        return "gateway_session_revoked", status.HTTP_401_UNAUTHORIZED
    return "gateway_unavailable", status.HTTP_503_SERVICE_UNAVAILABLE
