# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from rest_framework import status
from rest_framework.permissions import AllowAny
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.utils.login import user_login
from pi_dash.core.platform_federation import (
    PlatformAuthError,
    PlatformConfigurationError,
    PlatformFederationError,
    PlatformForbiddenError,
    consume_platform_session_token,
    platform_federation_enabled,
)
from pi_dash.db.models import Profile


class PlatformSessionEndpoint(APIView):
    """POST /auth/platform/session/"""

    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []

    def post(self, request):
        if not platform_federation_enabled():
            return Response({"error": "Platform federation is disabled"}, status=status.HTTP_404_NOT_FOUND)

        token = request.data.get("access_token") or request.data.get("token")
        authorization = request.headers.get("Authorization") or ""
        if not token and authorization.lower().startswith("bearer "):
            token = authorization[7:].strip()
        if not token:
            return Response({"error": "access_token is required"}, status=status.HTTP_400_BAD_REQUEST)

        try:
            user, workspace = consume_platform_session_token(token)
        except PlatformAuthError as exc:
            return Response({"error": str(exc)}, status=status.HTTP_401_UNAUTHORIZED)
        except PlatformForbiddenError as exc:
            return Response({"error": str(exc)}, status=status.HTTP_403_FORBIDDEN)
        except PlatformConfigurationError as exc:
            return Response({"error": str(exc)}, status=status.HTTP_503_SERVICE_UNAVAILABLE)
        except PlatformFederationError as exc:
            return Response({"error": str(exc)}, status=status.HTTP_400_BAD_REQUEST)

        user_login(request=request, user=user, is_app=True)
        profile, _ = Profile.objects.get_or_create(user=user)
        if profile.last_workspace_id != workspace.id:
            profile.last_workspace_id = workspace.id
            profile.save(update_fields=["last_workspace_id", "updated_at"])
        return Response(
            {
                "workspace": {
                    "id": str(workspace.id),
                    "slug": workspace.slug,
                    "name": workspace.name,
                },
                "redirect_url": f"/{workspace.slug}",
            },
            status=status.HTTP_200_OK,
        )
