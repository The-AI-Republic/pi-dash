# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Web-API endpoints for managing machine tokens (a.k.a. "connections" in
the UI). See ``.ai_design/n_runners_in_same_machine/design.md`` §5.

Mounted under ``/api/runners/machine-tokens/``:
- POST  — create a new MachineToken; returns the raw secret once.
- GET   — list active machine tokens for the calling user.

And per-token:
- POST .../revoke/ — flag the token revoked; cascades to owned runners.
"""

from __future__ import annotations

from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.models import MachineToken
from pi_dash.runner.services import tokens


class MachineTokenListCreateEndpoint(APIView):
    """List or create the calling user's machine tokens.

    A user clicking "Add Runner → New connection" in the UI hits this
    endpoint with the connection title and a workspace id. The response
    carries the freshly-minted secret exactly once; subsequent GETs
    return the same record without it.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request):
        title = (request.data.get("title") or "").strip()
        workspace_id = request.data.get("workspace")
        if not title:
            return Response(
                {"error": "title is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if len(title) > 128:
            return Response(
                {"error": "title must be at most 128 characters"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not workspace_id:
            return Response(
                {"error": "workspace is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        minted = tokens.mint_machine_token_secret()
        token = MachineToken.objects.create(
            workspace_id=workspace_id,
            created_by=request.user,
            title=title,
            secret_hash=minted.hashed,
            secret_fingerprint=minted.fingerprint,
        )
        return Response(
            {
                "token_id": str(token.id),
                "title": token.title,
                "fingerprint": token.secret_fingerprint,
                "secret": minted.raw,
                "created_at": token.created_at.isoformat(),
            },
            status=status.HTTP_201_CREATED,
        )

    def get(self, request):
        qs = (
            MachineToken.objects.filter(
                created_by=request.user,
                revoked_at__isnull=True,
            )
            .order_by("-created_at")
            .values(
                "id",
                "title",
                "workspace_id",
                "secret_fingerprint",
                "created_at",
                "last_seen_at",
            )
        )
        return Response(list(qs))


class MachineTokenRevokeEndpoint(APIView):
    """POST /api/runners/machine-tokens/<token_id>/revoke/.

    Flags the token revoked and cascades to its owned runners (each
    Runner.revoke() cancels in-flight runs and unpins queued ones).
    The daemon's WS, if up, sees the cascade fire and exits on next
    auth check; if down, the next reconnect fails 401.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, token_id):
        try:
            token = MachineToken.objects.get(id=token_id, created_by=request.user)
        except MachineToken.DoesNotExist:
            return Response(
                {"error": "token not found"},
                status=status.HTTP_404_NOT_FOUND,
            )
        if not token.is_active():
            return Response(
                {"error": "token already revoked"},
                status=status.HTTP_409_CONFLICT,
            )
        token.revoke()
        return Response(
            {
                "token_id": str(token.id),
                "revoked_at": token.revoked_at.isoformat(),
            }
        )
