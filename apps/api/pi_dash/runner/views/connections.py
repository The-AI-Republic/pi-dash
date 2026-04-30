# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""HTTP endpoints for the connection-first runner flow.

Web-app surface (session auth, mounted under ``/api/runners/connections/``):
    GET  /                       — list user's connections
    POST /                       — create a new connection (mints enrollment token)
    GET  /<id>/                  — detail
    PATCH /<id>/                 — rename
    POST /<id>/revoke/           — revoke (cascades to runners)

Daemon surface (``/api/v1/runner/connections/``):
    POST /enroll/                — public; exchange enrollment token for secret
    POST /<id>/runners/          — connection bearer; register a runner under the connection
    DELETE /<id>/runners/<rid>/  — connection bearer; deregister a runner
"""

from __future__ import annotations

import uuid as _uuid
from typing import Optional

from django.db import IntegrityError, transaction
from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import AllowAny, IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.authentication import ConnectionBearerAuthentication
from pi_dash.runner.models import (
    MAX_RUNNERS_PER_MACHINE,
    Connection,
    Pod,
    Runner,
)
from pi_dash.runner.serializers import (
    ConnectionSerializer,
    EnrollmentRequestSerializer,
    EnrollmentResponseSerializer,
    RunnerCreateRequestSerializer,
    RunnerSerializer,
)
from pi_dash.runner.services import tokens
from pi_dash.runner.services.permissions import is_workspace_member
from pi_dash.runner.services.pubsub import (
    close_runner_session,
    send_to_runner,
)


HEARTBEAT_INTERVAL_SECS = 25
PROTOCOL_VERSION = 3


class ConnectionListCreateEndpoint(APIView):
    """``GET/POST /api/runners/connections/``"""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request):
        qs = (
            Connection.objects.filter(created_by=request.user)
            .order_by("-created_at")
        )
        return Response(ConnectionSerializer(qs, many=True).data)

    def post(self, request):
        workspace_id_raw = request.data.get("workspace")
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
        if not is_workspace_member(request.user, workspace_id):
            return Response(
                {"error": "workspace not found"},
                status=status.HTTP_404_NOT_FOUND,
            )
        name = (request.data.get("name") or "").strip()[:128]
        enrollment = tokens.mint_enrollment_token()
        try:
            connection = Connection.objects.create(
                workspace_id=workspace_id,
                created_by=request.user,
                name=name,
                enrollment_token_hash=enrollment.hashed,
                enrollment_token_fingerprint=enrollment.fingerprint,
            )
        except IntegrityError:
            return Response(
                {"error": "connection name already in use"},
                status=status.HTTP_409_CONFLICT,
            )
        body = ConnectionSerializer(connection).data
        body["enrollment_token"] = enrollment.raw
        body["enrollment_expires_at"] = enrollment.expires_at.isoformat()
        return Response(body, status=status.HTTP_201_CREATED)


class ConnectionDetailEndpoint(APIView):
    """``GET/PATCH/DELETE /api/runners/connections/<connection_id>/``

    DELETE hard-removes the connection and every runner under it
    (``Runner.connection`` is ``on_delete=CASCADE``). It calls ``revoke()``
    first so the daemon receives a wire-level Revoke + in-flight runs
    are cancelled, then drops the row. AgentRuns hold ``runner`` as
    ``SET_NULL`` so historic rows survive the cascade with a null FK.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request, connection_id):
        connection = self._lookup(request.user, connection_id)
        if connection is None:
            return Response(
                {"error": "connection not found"},
                status=status.HTTP_404_NOT_FOUND,
            )
        return Response(ConnectionSerializer(connection).data)

    def patch(self, request, connection_id):
        connection = self._lookup(request.user, connection_id)
        if connection is None:
            return Response(
                {"error": "connection not found"},
                status=status.HTTP_404_NOT_FOUND,
            )
        new_name = (request.data.get("name") or "").strip()[:128]
        if not new_name:
            return Response(
                {"error": "name is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        connection.name = new_name
        try:
            connection.save(update_fields=["name"])
        except IntegrityError:
            return Response(
                {"error": "connection name already in use"},
                status=status.HTTP_409_CONFLICT,
            )
        return Response(ConnectionSerializer(connection).data)

    def delete(self, request, connection_id):
        connection = self._lookup(request.user, connection_id)
        if connection is None:
            return Response(
                {"error": "connection not found"},
                status=status.HTTP_404_NOT_FOUND,
            )
        if connection.is_active():
            connection.revoke()
        Connection.objects.filter(pk=connection.pk).delete()
        return Response(status=status.HTTP_204_NO_CONTENT)

    @staticmethod
    def _lookup(user, connection_id) -> Optional[Connection]:
        return Connection.objects.filter(
            id=connection_id, created_by=user
        ).first()


class ConnectionEnrollEndpoint(APIView):
    """``POST /api/v1/runner/connections/enroll/`` — public.

    Exchange a one-time enrollment token for the long-lived connection
    secret. The daemon stores the secret in its on-disk credentials and
    sends it on every WS connect (``Authorization: Bearer <secret>`` +
    ``X-Connection-Id``).
    """

    authentication_classes: list = []
    permission_classes = [AllowAny]

    @transaction.atomic
    def post(self, request):
        serializer = EnrollmentRequestSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        token_hash = tokens.hash_token(data["token"])
        try:
            connection = (
                Connection.objects.select_for_update()
                .get(enrollment_token_hash=token_hash)
            )
        except Connection.DoesNotExist:
            return Response(
                {"error": "invalid or expired enrollment token"},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        if connection.revoked_at is not None:
            return Response(
                {"error": "connection revoked"},
                status=status.HTTP_409_CONFLICT,
            )
        if connection.enrolled_at is not None:
            return Response(
                {"error": "enrollment token already used"},
                status=status.HTTP_409_CONFLICT,
            )

        secret = tokens.mint_connection_secret()
        connection.secret_hash = secret.hashed
        connection.secret_fingerprint = secret.fingerprint
        connection.enrolled_at = timezone.now()
        connection.host_label = (data.get("host_label") or "")[:255]
        # One-time token is consumed: clear the hash so it can't be reused.
        connection.enrollment_token_hash = ""
        connection.enrollment_token_fingerprint = ""
        connection.save(
            update_fields=[
                "secret_hash",
                "secret_fingerprint",
                "enrolled_at",
                "host_label",
                "enrollment_token_hash",
                "enrollment_token_fingerprint",
            ]
        )

        payload = EnrollmentResponseSerializer(
            {
                "connection_id": connection.id,
                "connection_secret": secret.raw,
                "workspace_slug": connection.workspace.slug,
                "heartbeat_interval_secs": HEARTBEAT_INTERVAL_SECS,
                "protocol_version": PROTOCOL_VERSION,
            }
        ).data
        return Response(payload, status=status.HTTP_201_CREATED)


class ConnectionRunnerListCreateEndpoint(APIView):
    """``GET/POST /api/v1/connections/<connection_id>/runners/``

    GET — list runners on this connection. Used by the daemon at startup
    to learn which runners to advertise.
    POST — register a new runner. The daemon mints the runner UUID
    locally (shared CLI/TUI util) and the cloud stores it.
    """

    authentication_classes = [ConnectionBearerAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def get(self, request, connection_id):
        connection = getattr(request, "auth_connection", None)
        if connection is None or str(connection.id) != str(connection_id):
            return Response(
                {"error": "forbidden"},
                status=status.HTTP_403_FORBIDDEN,
            )
        qs = connection.runners.filter(revoked_at__isnull=True).order_by("name")
        return Response(RunnerSerializer(qs, many=True).data)

    def post(self, request, connection_id):
        from pi_dash.db.models.project import Project

        connection = getattr(request, "auth_connection", None)
        if connection is None or str(connection.id) != str(connection_id):
            return Response(
                {"error": "forbidden"},
                status=status.HTTP_403_FORBIDDEN,
            )
        serializer = RunnerCreateRequestSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        project = Project.objects.filter(
            workspace_id=connection.workspace_id, identifier=data["project"]
        ).first()
        if project is None:
            return Response(
                {"error": "project not found in connection's workspace"},
                status=status.HTTP_404_NOT_FOUND,
            )

        pod_name = (data.get("pod") or "").strip()
        if pod_name:
            pod = Pod.objects.filter(
                project=project, name=pod_name, deleted_at__isnull=True
            ).first()
            if pod is None:
                pod = Pod.objects.filter(
                    project=project,
                    name=f"{project.identifier}_{pod_name}",
                    deleted_at__isnull=True,
                ).first()
            if pod is None:
                return Response(
                    {"error": f"pod {pod_name!r} not found in project"},
                    status=status.HTTP_404_NOT_FOUND,
                )
        else:
            pod = Pod.default_for_project_id(project.id)
            if pod is None:
                return Response(
                    {"error": "project has no default pod"},
                    status=status.HTTP_409_CONFLICT,
                )

        active_count = Runner.objects.filter(
            connection=connection, revoked_at__isnull=True
        ).count()
        if active_count >= MAX_RUNNERS_PER_MACHINE:
            return Response(
                {
                    "error": (
                        f"connection at capacity ({MAX_RUNNERS_PER_MACHINE} runners)"
                    )
                },
                status=status.HTTP_409_CONFLICT,
            )

        try:
            runner = Runner.objects.create(
                id=data["runner_id"],
                owner=connection.created_by,
                workspace=connection.workspace,
                pod=pod,
                name=data["name"],
                connection=connection,
                os=data["os"][:32],
                arch=data["arch"][:32],
                runner_version=data["version"][:32],
                protocol_version=data["protocol_version"],
            )
        except IntegrityError:
            return Response(
                {"error": "runner_name_taken"},
                status=status.HTTP_409_CONFLICT,
            )
        return Response(
            {
                "runner_id": str(runner.id),
                "pod_id": str(pod.id),
                "project_identifier": project.identifier,
            },
            status=status.HTTP_201_CREATED,
        )


class ConnectionRunnerDeleteEndpoint(APIView):
    """``DELETE /api/v1/connections/<connection_id>/runners/<runner_id>/``"""

    authentication_classes = [ConnectionBearerAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def delete(self, request, connection_id, runner_id):
        connection = getattr(request, "auth_connection", None)
        runner = getattr(request, "auth_runner", None)
        if (
            connection is None
            or str(connection.id) != str(connection_id)
            or runner is None
            or str(runner.id) != str(runner_id)
        ):
            return Response(
                {"error": "forbidden"},
                status=status.HTTP_403_FORBIDDEN,
            )
        runner.revoke()
        send_to_runner(
            runner.pk,
            {
                "type": "remove_runner",
                "runner_id": str(runner.id),
                "reason": "deregistered",
            },
        )
        close_runner_session(runner.pk)
        return Response({"ok": True})
