# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.views.base import BaseAPIView
from pi_dash.assistant.models import AssistantThread
from pi_dash.core.permissions import ROLE_MEMBER, workspace_role_by_slug


def role_error_response() -> Response:
    return Response(
        {"error": "role_not_allowed", "detail": "The assistant is available to workspace members."},
        status=status.HTTP_403_FORBIDDEN,
    )


class AssistantBaseView(BaseAPIView):
    """Assistant endpoints require workspace role >= MEMBER (guests excluded)."""

    def require_member(self, request, slug):
        role = workspace_role_by_slug(request.user, slug)
        if role is None or role < ROLE_MEMBER:
            return role_error_response()
        return None

    def owned_thread(self, request, slug, thread_id):
        return AssistantThread.objects.filter(
            id=thread_id, user=request.user, workspace__slug=slug
        ).first()
