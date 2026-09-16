# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""User-managed MCP tool servers for the assistant.

Per-user CRUD, scoped exactly like the BYOK config endpoints: a user only ever
sees and edits their own rows. The auth header is write-only — it is never
echoed back, only its presence (``has_auth_header``).
"""

from __future__ import annotations

from django.db import IntegrityError
from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.views.base import BaseAPIView
from pi_dash.assistant import crypto, ssrf
from pi_dash.assistant.errors import AssistantError
from pi_dash.assistant.models import AssistantMCPServer
from pi_dash.assistant.runtime import mcp
from pi_dash.assistant.serializers import AssistantMCPServerSerializer


def _serialize(server: AssistantMCPServer, effective_prefix: str | None = None) -> dict:
    """Serialize a row, optionally with the prefix its tools actually get.

    ``tool_prefix`` is the row's own slug; the prefix a run assigns can differ,
    because slugification is lossy and colliding servers are disambiguated with
    a counter. Showing the raw slug tells two colliding servers they share a
    prefix when they do not, so the list view resolves it (see ``get``).
    """
    return {**AssistantMCPServerSerializer(server).data, "effective_tool_prefix": effective_prefix}


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
        servers = list(AssistantMCPServer.objects.filter(user=request.user))
        # Resolved over the *enabled* set, exactly as a run does — a disabled
        # server claims no prefix, so it cannot push an enabled one onto a
        # counter suffix it would never actually get.
        effective = mcp.unique_prefixes([s for s in servers if s.is_enabled])
        return Response([_serialize(s, effective.get(s.pk)) for s in servers])

    def post(self, request):
        serializer = AssistantMCPServerSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        if ssrf.is_blocked(data["url"]):
            return _blocked_response()

        if AssistantMCPServer.objects.filter(user=request.user, name=data["name"]).exists():
            return _duplicate_name_response()

        # Toolsets are entered sequentially at the start of every turn, each
        # bounded only by the connect timeout, so an unbounded server list is
        # dead time the user pays on every message they send. Refuse here
        # rather than let the run-time cap silently drop the newest server.
        limit = mcp.max_servers()
        if AssistantMCPServer.objects.filter(user=request.user).count() >= limit:
            return Response(
                {
                    "error": "too_many_servers",
                    "detail": f"You can have at most {limit} tool servers. Remove one before adding another.",
                },
                status=status.HTTP_400_BAD_REQUEST,
            )

        server = AssistantMCPServer(
            user=request.user,
            name=data["name"],
            url=data["url"],
            is_enabled=data.get("is_enabled", True),
        )
        error = _apply_auth_header(server, data.get("auth_header"))
        if error is not None:
            return error
        try:
            server.save()
        except IntegrityError:
            # Concurrent create with the same name: the pre-check above raced.
            # The unique constraint is the authority; answer as the pre-check
            # would have.
            return _duplicate_name_response()
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
        try:
            server.save()
        except IntegrityError:
            return _duplicate_name_response()
        return Response(_serialize(server))

    def delete(self, request, server_id):
        server = self._owned(request, server_id)
        if server is None:
            return Response(status=status.HTTP_404_NOT_FOUND)
        server.delete()
        return Response(status=status.HTTP_204_NO_CONTENT)
