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

import uuid as _uuid

from django.db import IntegrityError
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.models import MachineToken, Runner
from pi_dash.runner.services import tokens
from pi_dash.runner.services.permissions import is_workspace_member


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
        workspace_id_raw = request.data.get("workspace")
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
        if not workspace_id_raw:
            return Response(
                {"error": "workspace is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        try:
            workspace_id = _uuid.UUID(str(workspace_id_raw))
        except (ValueError, AttributeError):
            return Response(
                {"error": "workspace must be a UUID"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        # Authz: only members of the target workspace can mint a token in it.
        # Without this gate, any authenticated user could mint a token in any
        # workspace whose UUID they can guess and use it to register runners.
        if not is_workspace_member(request.user, workspace_id):
            return Response(
                {"error": "workspace not found"},
                status=status.HTTP_404_NOT_FOUND,
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


class TokenRunnerCreateEndpoint(APIView):
    """POST /api/v1/runner/register-under-token/.

    Token-authenticated endpoint that registers an *additional* runner
    under an existing MachineToken. The daemon calls this when a user
    runs ``pidash configure runner --name <NAME>`` on a machine that
    already has a token installed.

    Auth headers (same as the WS upgrade):
        X-Token-Id:    <token_id>
        Authorization: Bearer <token_secret>

    Body:
        { "name": "<NAME>", "os": "...", "arch": "...",
          "version": "...", "protocol_version": <int> }

    Response:
        { "runner_id": "<UUID>" }

    Token-auth runners never present a per-runner bearer secret on the
    wire (the WS auths as the token; runner_id is just a routing key),
    so no `credential_secret` is minted or returned. Persisted
    `credential_hash` is a non-empty placeholder so the unique-by-hash
    legacy path stays well-formed for any future tooling that walks it.
    """

    authentication_classes: list = []
    permission_classes: list = []

    def post(self, request):
        token_id_raw = request.headers.get("X-Token-Id", "")
        auth = request.headers.get("Authorization", "")
        if not token_id_raw or not auth.lower().startswith("bearer "):
            return Response(
                {"error": "missing X-Token-Id or Authorization header"},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        secret_raw = auth.split(" ", 1)[1].strip()

        try:
            token_id = _uuid.UUID(token_id_raw)
        except (ValueError, AttributeError):
            return Response(
                {"error": "invalid X-Token-Id"},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        secret_hash = tokens.hash_token(secret_raw)
        token = MachineToken.objects.filter(
            id=token_id, secret_hash=secret_hash, revoked_at__isnull=True
        ).first()
        if token is None:
            return Response(
                {"error": "invalid or revoked token"},
                status=status.HTTP_401_UNAUTHORIZED,
            )

        name = (request.data.get("name") or "").strip()
        if not name or len(name) > 128:
            return Response(
                {"error": "name is required and must be 1..128 chars"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        # Cap check uses the same per-machine limit the daemon enforces
        # locally (design.md §16). Cloud rejects beyond it as a defence
        # in depth — a tampered daemon couldn't blow past the cap by
        # repeating registrations.
        active_count = Runner.objects.filter(
            machine_token=token, revoked_at__isnull=True
        ).count()
        from pi_dash.runner.models import MAX_RUNNERS_PER_MACHINE

        cap = MAX_RUNNERS_PER_MACHINE
        if active_count >= cap:
            return Response(
                {"error": f"machine token at capacity ({cap} runners)"},
                status=status.HTTP_409_CONFLICT,
            )

        # Token-auth runners don't carry their own bearer; the WS auths
        # as the token. We still write a non-empty `credential_hash` so
        # the column never sees an empty string (it's nullable=False)
        # and any future legacy-path tooling that scans by hash sees a
        # well-formed row. The placeholder is salted with the runner_id
        # so collisions across runners can't happen.
        try:
            placeholder_runner_id = _uuid.uuid4()
            placeholder_hash = tokens.hash_token(
                f"token-auth:{token.id}:{placeholder_runner_id}"
            )
            runner = Runner.objects.create(
                id=placeholder_runner_id,
                owner=token.created_by,
                workspace=token.workspace,
                name=name,
                credential_hash=placeholder_hash,
                credential_fingerprint="token-auth",
                machine_token=token,
                os=(request.data.get("os") or "")[:32],
                arch=(request.data.get("arch") or "")[:32],
                runner_version=(request.data.get("version") or "")[:32],
                protocol_version=int(request.data.get("protocol_version") or 2),
            )
        except IntegrityError:
            # `UNIQUE(pod, name)` (or another DB-level constraint) collision.
            # Return a generic message so we don't leak constraint names or
            # internal exception details to the daemon.
            return Response(
                {"error": "runner_name_taken"},
                status=status.HTTP_409_CONFLICT,
            )

        return Response(
            {"runner_id": str(runner.id)},
            status=status.HTTP_201_CREATED,
        )


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
