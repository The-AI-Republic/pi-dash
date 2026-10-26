# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Desktop sign-in handoff endpoints.

* ``POST /api/v1/auth/desktop-handoff/`` — ``Authorization: Bearer dhk_...``.
  The CLI (driven by the desktop app) trades its handoff key for a
  single-use, 60-second code and an absolute ``redeem_url``.
* ``GET <DESKTOP_HANDOFF_REDEEM_PATH>?code=...`` — the desktop webview
  opens this; it consumes the code, logs the user into a normal Django
  session and redirects to the app.

See :mod:`pi_dash.authentication.services.desktop_handoff`.
"""

# Python imports
from urllib.parse import urlencode

# Django imports
from django.conf import settings
from django.http import HttpResponseRedirect
from django.views import View

# Third party imports
from rest_framework import status
from rest_framework.permissions import AllowAny
from rest_framework.response import Response
from rest_framework.throttling import AnonRateThrottle
from rest_framework.views import APIView

# Module imports
from pi_dash.authentication.services.desktop_handoff import (
    consume_handoff_code,
    create_handoff_code,
)
from pi_dash.authentication.utils.host import base_host
from pi_dash.authentication.utils.login import user_login
from pi_dash.runner.services.dev_machines import (
    InvalidMachineFingerprint,
    parse_machine_fingerprint,
)


class DesktopHandoffThrottle(AnonRateThrottle):
    """Per-IP cap on code minting; the legitimate flow needs one per sign-in."""

    scope = "auth_desktop_handoff"


def _bearer(request) -> str:
    header = request.headers.get("Authorization") or ""
    scheme, _, value = header.partition(" ")
    if scheme.lower() != "bearer":
        return ""
    return value.strip()


class DesktopHandoffCodeEndpoint(APIView):
    """Trade a ``dhk_`` handoff key for a short-lived redeem code."""

    authentication_classes: list = []
    permission_classes = [AllowAny]
    throttle_classes = [DesktopHandoffThrottle]

    def post(self, request):
        try:
            fingerprint = parse_machine_fingerprint(request.data.get("machine_fingerprint"))
        except InvalidMachineFingerprint:
            return Response(
                {"error": "invalid_machine_fingerprint"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        result = create_handoff_code(raw_key=_bearer(request), machine_fingerprint=fingerprint or None)
        if result is None:
            return Response(
                {"error": "invalid_handoff_key"},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        code, ttl = result
        redeem_url = request.build_absolute_uri(f"{settings.DESKTOP_HANDOFF_REDEEM_PATH}?{urlencode({'code': code})}")
        return Response(
            {"code": code, "expires_in": ttl, "redeem_url": redeem_url},
            status=status.HTTP_201_CREATED,
        )


class DesktopHandoffRedeemEndpoint(View):
    """Consume a handoff code and start a browser session."""

    def get(self, request):
        app = base_host(request=request, is_app=True).rstrip("/")
        user = consume_handoff_code((request.GET.get("code") or "").strip())
        if user is None:
            return HttpResponseRedirect(f"{app}/sign-in?{urlencode({'error': 'handoff_invalid'})}")
        user_login(request=request, user=user, is_app=True)
        return HttpResponseRedirect(f"{app}/")
