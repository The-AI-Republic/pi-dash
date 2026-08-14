# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""User-managed MCP tool servers for the assistant.

Per-user CRUD, scoped exactly like the BYOK config endpoints: a user only ever
sees and edits their own rows. The auth header is write-only — it is never
echoed back, only its presence (``has_auth_header``).
"""

from __future__ import annotations

from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.views.base import BaseAPIView
from pi_dash.assistant import crypto, ssrf
from pi_dash.assistant.errors import AssistantError
from pi_dash.assistant.models import AssistantMCPServer
from pi_dash.assistant.serializers import AssistantMCPServerSerializer


def _serialize(server: AssistantMCPServer) -> dict:
    return AssistantMCPServerSerializer(server).data


def _blocked_response() -> Response:
    return Response(
        {"error": "url_blocked", "detail": "That server host is not allowed."},
        status=status.HTTP_400_BAD_REQUEST,
    )


def _duplicate_name_response() -> Response:
    return Response(
        {"error": "duplicate_name", "detail": "You already have a tool server with that name."},
        status=status.HTTP_400_BAD_REQUEST,
    )


def _apply_auth_header(server: AssistantMCPServer, auth_header: str | None) -> Response | None:
    """Set/clear the encrypted auth header. Returns an error Response or None.

    An explicitly empty string clears the stored header; omitting the field
    entirely leaves it untouched (so a PATCH that only renames the server does
    not silently drop its credential).
    """
    if auth_header is None:
        return None
    if auth_header == "":
        server.auth_header_encrypted = None
        return None
    try:
        server.auth_header_encrypted = crypto.encrypt(auth_header)
    except AssistantError as exc:
        return Response({"error": exc.code, "detail": exc.detail}, status=exc.http_status)
    return None


class AssistantMCPServerListCreateEndpoint(BaseAPIView):
    def get(self, request):
        servers = AssistantMCPServer.objects.filter(user=request.user)
        return Response([_serialize(s) for s in servers])

    def post(self, request):
        serializer = AssistantMCPServerSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        if ssrf.is_blocked(data["url"]):
            return _blocked_response()

        if AssistantMCPServer.objects.filter(user=request.user, name=data["name"]).exists():
            return _duplicate_name_response()

        server = AssistantMCPServer(
            user=request.user,
            name=data["name"],
            url=data["url"],
            is_enabled=data.get("is_enabled", True),
        )
        error = _apply_auth_header(server, data.get("auth_header"))
        if error is not None:
            return error
        server.save()
        return Response(_serialize(server), status=status.HTTP_201_CREATED)


class AssistantMCPServerDetailEndpoint(BaseAPIView):
    def _owned(self, request, server_id):
        return AssistantMCPServer.objects.filter(id=server_id, user=request.user).first()

    def patch(self, request, server_id):
        server = self._owned(request, server_id)
        if server is None:
            return Response(status=status.HTTP_404_NOT_FOUND)

        serializer = AssistantMCPServerSerializer(instance=server, data=request.data, partial=True)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        url = data.get("url", server.url)
        if url != server.url and ssrf.is_blocked(url):
            return _blocked_response()

        name = data.get("name", server.name)
        if name != server.name and (
            AssistantMCPServer.objects.filter(user=request.user, name=name)
            .exclude(pk=server.pk)
            .exists()
        ):
            return _duplicate_name_response()

        for field in ("name", "url", "is_enabled"):
            if field in data:
                setattr(server, field, data[field])
        error = _apply_auth_header(server, data.get("auth_header"))
        if error is not None:
            return error
        server.save()
        return Response(_serialize(server))

    def delete(self, request, server_id):
        server = self._owned(request, server_id)
        if server is None:
            return Response(status=status.HTTP_404_NOT_FOUND)
        server.delete()
        return Response(status=status.HTTP_204_NO_CONTENT)
