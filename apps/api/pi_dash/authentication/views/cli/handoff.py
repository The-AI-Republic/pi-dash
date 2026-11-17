# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Desktop session handoff endpoints (desktop_unified_signin §5).

* ``POST /api/v1/auth/desktop-handoff/`` — the daemon asks for a single-use
  code with a workspace daemon credential and a ``challenge``; the server
  stores the code bound to the user, the machine and the challenge, and
  returns it with the account it belongs to.
* ``POST /api/v1/auth/desktop-handoff/redeem/`` — the app redeems the code
  with its verifier (never a URL) and ``expected_user_id``; on success the
  server creates the same kind of session a desktop browser sign-in
  creates (the OSS session, marked as a desktop client) and its
  ``Set-Cookie`` goes out on the response. On any mismatch the code is
  consumed and no session is created.

The verifier never leaves the app's process: issue takes only its hash
(the challenge), redeem recomputes the hash and compares. Neither the code
nor the verifier is logged; every issue and redeem is audited with user,
machine and outcome.
"""

import base64
import binascii
import logging

from rest_framework import status
from rest_framework.permissions import AllowAny, IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.api.middleware.api_authentication import APIKeyAuthentication
from pi_dash.authentication.utils.login import user_login
from pi_dash.ee.authentication.desktop import DESKTOP_CLIENT, DESKTOP_SESSION_KEY
from pi_dash.runner.services import desktop_handoff

logger = logging.getLogger("pi_dash.auth.cli.handoff")

#: Bound on the challenge (43 chars for base64url SHA-256); longer is junk.
_MAX_CHALLENGE_LEN = 128
#: Bound on the verifier (32 bytes); longer is not what the app sends.
_MAX_VERIFIER_BYTES = 64


def _decode_verifier(raw) -> bytes | None:
    """The verifier bytes from a base64url body field, or ``None``.

    Strict: anything outside the urlsafe alphabet fails here rather than
    being silently trimmed into a different verifier.
    """
    if not isinstance(raw, str) or not raw.strip():
        return None
    text = raw.strip()
    try:
        return base64.b64decode(text + "=" * (-len(text) % 4), altchars=b"-_", validate=True)
    except (binascii.Error, ValueError):
        return None


class DesktopHandoffIssueEndpoint(APIView):
    """``POST /api/v1/auth/desktop-handoff/`` — the daemon asks for a code.

    Authenticated with one of the daemon's workspace credentials
    (``X-Api-Key`` machine token). The code is bound to that machine and
    answered with the account it belongs to, so the app can compare it
    against the daemon's ``Hello`` before redeeming.
    """

    permission_classes = [IsAuthenticated]
    authentication_classes = [APIKeyAuthentication]

    def post(self, request):
        machine_token = getattr(request, "auth_machine_token", None)
        if machine_token is None or machine_token.dev_machine_id is None:
            return Response(
                {"error": "machine_token_required", "error_description": "Issue a handoff with a daemon credential."},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        challenge = str(request.data.get("challenge") or "").strip()
        if not challenge or len(challenge) > _MAX_CHALLENGE_LEN:
            return Response(
                {"error": "invalid_request", "error_description": "challenge is required."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        user = request.user
        machine = machine_token.dev_machine
        if not user.is_active:
            return Response({"error": "invalid_grant"}, status=status.HTTP_400_BAD_REQUEST)
        try:
            row = desktop_handoff.issue_handoff(user=user, dev_machine=machine, challenge=challenge)
        except desktop_handoff.HandoffError as exc:
            return Response(
                {"error": exc.code, "error_description": "The handoff is unavailable right now."},
                status=exc.http_status,
            )
        return Response(
            {
                "code": row._raw,
                "user_id": str(user.id),
                "email": user.email,
                "expires_in": int(desktop_handoff.HANDOFF_TTL.total_seconds()),
            },
            status=status.HTTP_200_OK,
        )


class DesktopHandoffRedeemEndpoint(APIView):
    """``POST /api/v1/auth/desktop-handoff/redeem/`` — the app redeems a code.

    Body ``{code, verifier, expected_user_id}`` (verifier base64url). Four
    checks before any session (design §5 step 6): the code is live and
    unused; SHA-256(verifier) matches the stored challenge; the code's user
    is ``expected_user_id``; the user and the machine are active. Only then
    is the code consumed and the OSS session created — the same session a
    desktop browser sign-in creates, marked ``client = "desktop"``. On any
    mismatch the code is consumed and nothing is created.
    """

    permission_classes = [AllowAny]
    authentication_classes: list = []

    def post(self, request):
        code = str(request.data.get("code") or "").strip()
        expected_user_id = str(request.data.get("expected_user_id") or "").strip()
        verifier = _decode_verifier(request.data.get("verifier"))
        if not code or verifier is None or not expected_user_id:
            return Response(
                {"error": "invalid_request", "error_description": "code, verifier and expected_user_id are required."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if len(verifier) > _MAX_VERIFIER_BYTES:
            return Response({"error": "invalid_request"}, status=status.HTTP_400_BAD_REQUEST)
        if not desktop_handoff.handoff_enabled():
            return Response(
                {
                    "error": desktop_handoff.HANDOFF_DISABLED,
                    "error_description": "The handoff is unavailable right now.",
                },
                status=status.HTTP_410_GONE,
            )
        try:
            payload = desktop_handoff.redeem_handoff(code=code, verifier=verifier, expected_user_id=expected_user_id)
        except desktop_handoff.HandoffError as exc:
            return Response({"error": exc.code}, status=exc.http_status)
        user = payload["user"]
        user_login(request=request, user=user)
        # This is the same session a desktop browser sign-in creates,
        # marked as a desktop client (design §5 step 6).
        request.session[DESKTOP_SESSION_KEY] = DESKTOP_CLIENT
        return Response(
            {"user_id": payload["user_id"], "email": payload["email"]},
            status=status.HTTP_200_OK,
        )
