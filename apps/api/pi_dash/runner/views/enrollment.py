# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Runner-as-trust-unit enrollment + refresh endpoints.

See ``.ai_design/move_to_https/design.md`` §5. The web UI mints a
one-time enrollment token attached to a Runner row in PENDING state;
the daemon redeems it for a refresh token + access token via
``POST /api/v1/runner/runners/enroll/``. Subsequent refreshes hit
``POST /api/v1/runner/runners/<rid>/refresh/`` and rotate the refresh
token in lock-step on the server.
"""

from __future__ import annotations

import logging
import uuid as _uuid
from typing import Optional

from django.db import IntegrityError, transaction
from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import AllowAny, IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.authentication import RunnerRefreshTokenAuthentication
from pi_dash.runner.models import (
    MachineToken,
    Pod,
    Runner,
    RunnerForceRefresh,
)
from pi_dash.runner.serializers import (
    RunnerEnrollRequestSerializer,
    RunnerEnrollmentInviteSerializer,
)
from pi_dash.runner.services import tokens
from pi_dash.runner.services.permissions import is_workspace_member

logger = logging.getLogger(__name__)


def _maybe_mint_machine_token(
    *, user, workspace, host_label: str
) -> Optional[tokens.MintedToken]:
    """Bootstrap a MachineToken if the user has none for this host.

    ``design.md`` §5.1: bootstrap runs inside the enrollment transaction
    so two concurrent enrollments cannot both mint a token. The unique
    constraint backs us up; the lock prevents the steady-state race.
    """
    locked = (
        MachineToken.objects.select_for_update()
        .filter(
            user=user,
            workspace=workspace,
            host_label=host_label,
            revoked_at__isnull=True,
        )
        .first()
    )
    if locked is not None:
        return None
    minted = tokens.mint_machine_token()
    try:
        MachineToken.objects.create(
            user=user,
            workspace=workspace,
            host_label=host_label,
            token_hash=minted.hashed,
            token_fingerprint=minted.fingerprint,
            label=f"machine: {host_label[:96]}",
            is_service=True,
        )
    except IntegrityError:
        return None
    return minted


class RunnerInviteEndpoint(APIView):
    """``POST /api/runners/invites/`` — web UI mints a runner enrollment token.

    Creates a new ``Runner`` row in PENDING state (no refresh token yet)
    and returns a one-time enrollment token plus the runner row's
    ``runner_id`` so the daemon can identify itself on enroll.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request):
        workspace_id_raw = request.data.get("workspace")
        project_identifier = (request.data.get("project") or "").strip()
        if not workspace_id_raw or not project_identifier:
            return Response(
                {"error": "workspace and project are required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        try:
            workspace_id = _uuid.UUID(str(workspace_id_raw))
        except (ValueError, AttributeError):
            return Response(
                {"error": "workspace must be a UUID"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not is_workspace_member(request.user, workspace_id):
            return Response(
                {"error": "workspace not found"},
                status=status.HTTP_404_NOT_FOUND,
            )
        from pi_dash.db.models.project import Project

        project = Project.objects.filter(
            workspace_id=workspace_id, identifier=project_identifier
        ).first()
        if project is None:
            return Response(
                {"error": "project not found in workspace"},
                status=status.HTTP_404_NOT_FOUND,
            )
        pod_name = (request.data.get("pod") or "").strip()
        pod: Optional[Pod] = None
        if pod_name:
            pod = Pod.objects.filter(
                project=project, name=pod_name, deleted_at__isnull=True
            ).first()
        if pod is None:
            pod = Pod.default_for_project_id(project.id)
        if pod is None:
            return Response(
                {"error": "project has no default pod"},
                status=status.HTTP_409_CONFLICT,
            )

        name = (request.data.get("name") or "").strip()[:128]
        if not name:
            count = Runner.objects.filter(pod=pod).count()
            name = f"runner_{count + 1:03d}"

        enrollment = tokens.mint_enrollment_token()
        try:
            runner = Runner.objects.create(
                owner=request.user,
                workspace_id=workspace_id,
                pod=pod,
                name=name,
                enrollment_token_hash=enrollment.hashed,
                enrollment_token_fingerprint=enrollment.fingerprint,
            )
        except IntegrityError:
            return Response(
                {"error": "runner_name_taken"},
                status=status.HTTP_409_CONFLICT,
            )

        body = RunnerEnrollmentInviteSerializer(
            {
                "runner_id": runner.id,
                "name": runner.name,
                "workspace_slug": runner.workspace.slug,
                "project_identifier": project.identifier,
                "pod_id": pod.id,
                "enrollment_token": enrollment.raw,
                "enrollment_expires_at": enrollment.expires_at.isoformat(),
            }
        ).data
        return Response(body, status=status.HTTP_201_CREATED)


class RunnerEnrollEndpoint(APIView):
    """``POST /api/v1/runner/runners/enroll/`` — public.

    Exchanges a one-time enrollment token for the runner's long-lived
    refresh token and a short-lived access token. Bootstraps a
    ``MachineToken`` for ``(user, workspace, host_label)`` if none
    exists; otherwise the response omits ``machine_token``.
    """

    authentication_classes: list = []
    permission_classes = [AllowAny]
    throttle_classes: list = []

    def post(self, request):
        serializer = RunnerEnrollRequestSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        token_hash = tokens.hash_token(data["enrollment_token"])
        host_label = (data.get("host_label") or "")[:255]
        body_name = (data.get("name") or "").strip()[:128]

        with transaction.atomic():
            try:
                runner = (
                    Runner.objects.select_for_update()
                    .select_related("workspace", "pod__project")
                    .get(enrollment_token_hash=token_hash)
                )
            except Runner.DoesNotExist:
                return Response(
                    {"error": "invalid_or_expired_enrollment_token"},
                    status=status.HTTP_401_UNAUTHORIZED,
                )
            if runner.revoked_at is not None:
                return Response(
                    {"error": "runner_revoked"},
                    status=status.HTTP_409_CONFLICT,
                )
            if runner.enrolled_at is not None:
                return Response(
                    {"error": "enrollment_token_already_used"},
                    status=status.HTTP_409_CONFLICT,
                )

            refresh = tokens.mint_refresh_token()
            access = tokens.mint_access_token(
                runner_id=str(runner.id),
                user_id=str(runner.owner_id),
                workspace_id=str(runner.workspace_id),
                rtg=1,
            )
            update_fields = [
                "host_label",
                "enrolled_at",
                "enrollment_token_hash",
                "enrollment_token_fingerprint",
                "refresh_token_hash",
                "refresh_token_fingerprint",
                "refresh_token_generation",
                "previous_refresh_token_hash",
            ]
            runner.host_label = host_label or runner.host_label
            runner.enrolled_at = timezone.now()
            runner.enrollment_token_hash = ""
            runner.enrollment_token_fingerprint = ""
            runner.refresh_token_hash = refresh.hashed
            runner.refresh_token_fingerprint = refresh.fingerprint
            runner.refresh_token_generation = 1
            runner.previous_refresh_token_hash = ""
            if body_name:
                runner.name = body_name
                update_fields.append("name")
            runner.save(update_fields=update_fields)

            # MachineToken bootstrap inside the same transaction.
            machine_minted: Optional[tokens.MintedToken] = None
            if host_label:
                machine_minted = _maybe_mint_machine_token(
                    user=runner.owner,
                    workspace=runner.workspace,
                    host_label=host_label,
                )

        project_identifier = (
            runner.pod.project.identifier
            if runner.pod and runner.pod.project_id
            else ""
        )
        body = {
            "runner_id": str(runner.id),
            "runner_name": runner.name,
            "refresh_token": refresh.raw,
            "access_token": access.raw,
            "access_token_expires_at": access.expires_at.isoformat(),
            "refresh_token_generation": runner.refresh_token_generation,
            "workspace_slug": runner.workspace.slug,
            "pod_slug": runner.pod.name if runner.pod_id else "",
            "project_identifier": project_identifier,
            "long_poll_interval_secs": 25,
            "protocol_version": 4,
            "machine_token_minted": machine_minted is not None,
        }
        if machine_minted is not None:
            body["machine_token"] = machine_minted.raw
        return Response(body, status=status.HTTP_201_CREATED)


class RunnerRefreshEndpoint(APIView):
    """``POST /api/v1/runner/runners/<runner_id>/refresh/``.

    Verifies the bearer **refresh** token against the runner's
    ``refresh_token_hash`` (current) and ``previous_refresh_token_hash``
    (replay window), rotates atomically under ``select_for_update``,
    and returns a fresh refresh+access token pair. Live workspace
    membership re-check happens here per ``design.md`` §5.3 step 4.
    """

    authentication_classes = [RunnerRefreshTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def post(self, request, runner_id):
        raw = getattr(request, "auth_refresh_token", None)
        if not raw:
            return Response(
                {"error": "missing_refresh_token"},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        presented_hash = tokens.hash_token(raw)

        with transaction.atomic():
            runner = (
                Runner.objects.select_for_update()
                .select_related("workspace")
                .filter(id=runner_id)
                .first()
            )
            if runner is None:
                return Response(
                    {"error": "invalid_refresh_token"},
                    status=status.HTTP_401_UNAUTHORIZED,
                )
            if runner.revoked_at is not None:
                return Response(
                    {"error": "runner_revoked"},
                    status=status.HTTP_401_UNAUTHORIZED,
                )

            if presented_hash == runner.refresh_token_hash:
                pass  # Current generation; happy path.
            elif (
                runner.previous_refresh_token_hash
                and presented_hash == runner.previous_refresh_token_hash
            ):
                runner.revoke(reason="refresh_token_replayed")
                return Response(
                    {"error": "refresh_token_replayed"},
                    status=status.HTTP_401_UNAUTHORIZED,
                )
            else:
                return Response(
                    {"error": "invalid_refresh_token"},
                    status=status.HTTP_401_UNAUTHORIZED,
                )

            if not is_workspace_member(runner.owner, runner.workspace_id):
                runner.revoke(reason="membership_revoked")
                return Response(
                    {"error": "membership_revoked"},
                    status=status.HTTP_401_UNAUTHORIZED,
                )

            new_refresh = tokens.mint_refresh_token()
            runner.previous_refresh_token_hash = runner.refresh_token_hash
            runner.refresh_token_hash = new_refresh.hashed
            runner.refresh_token_fingerprint = new_refresh.fingerprint
            runner.refresh_token_generation = (
                runner.refresh_token_generation + 1
            )
            runner.save(
                update_fields=[
                    "previous_refresh_token_hash",
                    "refresh_token_hash",
                    "refresh_token_fingerprint",
                    "refresh_token_generation",
                ]
            )
            new_access = tokens.mint_access_token(
                runner_id=str(runner.id),
                user_id=str(runner.owner_id),
                workspace_id=str(runner.workspace_id),
                rtg=runner.refresh_token_generation,
            )
            RunnerForceRefresh.objects.filter(runner=runner).delete()

        return Response(
            {
                "refresh_token": new_refresh.raw,
                "access_token": new_access.raw,
                "access_token_expires_at": new_access.expires_at.isoformat(),
                "refresh_token_generation": runner.refresh_token_generation,
            }
        )


class MachineTokenTicketEndpoint(APIView):
    """``POST /api/v1/workspaces/<ws>/machine-tokens/tickets/``.

    Web UI mints a one-time ticket the CLI can redeem for a
    MachineToken via ``pidash auth login <ticket>``. Backed by Redis
    with ``EX 60`` (``design.md`` §5.6, ``tasks.md`` §1.5).
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, workspace_id):
        try:
            ws_uuid = _uuid.UUID(str(workspace_id))
        except (ValueError, AttributeError):
            return Response(
                {"error": "invalid_workspace_id"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not is_workspace_member(request.user, ws_uuid):
            return Response(
                {"error": "workspace not found"},
                status=status.HTTP_404_NOT_FOUND,
            )
        host_label = (request.data.get("host_label") or "").strip()[:255]
        if not host_label:
            return Response(
                {"error": "host_label is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        ticket = _uuid.uuid4().hex
        from pi_dash.settings.redis import redis_instance

        client = redis_instance()
        if client is not None:
            import json as _json

            client.set(
                f"machine_token_ticket:{ticket}",
                _json.dumps(
                    {
                        "user_id": str(request.user.id),
                        "workspace_id": str(ws_uuid),
                        "host_label": host_label,
                    }
                ),
                ex=60,
            )
        return Response(
            {"ticket": ticket, "expires_in_secs": 60},
            status=status.HTTP_201_CREATED,
        )


class MachineTokenRedeemEndpoint(APIView):
    """``POST /api/v1/runner/machine-tokens/`` — public; redeem a ticket."""

    authentication_classes: list = []
    permission_classes = [AllowAny]
    throttle_classes: list = []

    def post(self, request):
        ticket = (request.data.get("ticket") or "").strip()
        if not ticket:
            return Response(
                {"error": "ticket is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        from pi_dash.settings.redis import redis_instance

        client = redis_instance()
        if client is None:
            return Response(
                {"error": "redis_unavailable"},
                status=status.HTTP_503_SERVICE_UNAVAILABLE,
            )
        key = f"machine_token_ticket:{ticket}"
        # GETDEL: atomic redeem (consume on read).
        try:
            blob = client.execute_command("GETDEL", key)
        except Exception:
            blob = client.get(key)
            if blob is not None:
                client.delete(key)
        if not blob:
            return Response(
                {"error": "invalid_or_expired_ticket"},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        if isinstance(blob, bytes):
            blob = blob.decode()
        import json as _json

        try:
            payload = _json.loads(blob)
        except (TypeError, ValueError):
            return Response(
                {"error": "invalid_ticket_payload"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        from pi_dash.db.models import User
        from pi_dash.db.models.workspace import Workspace

        try:
            user = User.objects.get(id=payload["user_id"])
            workspace = Workspace.objects.get(id=payload["workspace_id"])
        except (User.DoesNotExist, Workspace.DoesNotExist, KeyError):
            return Response(
                {"error": "stale_ticket"},
                status=status.HTTP_410_GONE,
            )
        host_label = (payload.get("host_label") or "")[:255]

        with transaction.atomic():
            minted = _maybe_mint_machine_token(
                user=user, workspace=workspace, host_label=host_label
            )
        if minted is None:
            return Response(
                {"error": "machine_token_already_active"},
                status=status.HTTP_409_CONFLICT,
            )
        return Response(
            {
                "machine_token": minted.raw,
                "host_label": host_label,
                "workspace_slug": workspace.slug,
            },
            status=status.HTTP_201_CREATED,
        )
