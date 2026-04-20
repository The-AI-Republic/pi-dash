# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import transaction
from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import AllowAny, IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.runner.authentication import RunnerBearerAuthentication
from pi_dash.runner.models import (
    Runner,
    RunnerRegistrationToken,
    RunnerStatus,
)
from pi_dash.runner.serializers import (
    RegistrationRequestSerializer,
    RegistrationResponseSerializer,
    RegistrationTokenSerializer,
)
from pi_dash.runner.services import tokens
from pi_dash.runner.services.matcher import can_register_another
from pi_dash.runner.services.pubsub import close_runner_session


HEARTBEAT_INTERVAL_SECS = 25
PROTOCOL_VERSION = 1


class HealthEndpoint(APIView):
    authentication_classes: list = []
    permission_classes = [AllowAny]

    def get(self, request):
        return Response({"ok": True, "protocol_version": PROTOCOL_VERSION})


class RegisterEndpoint(APIView):
    """POST /api/v1/runner/register/ — one-time-token to runner-secret exchange.

    Called by the daemon during ``pidash configure``.
    """

    authentication_classes: list = []
    permission_classes = [AllowAny]

    @transaction.atomic
    def post(self, request):
        serializer = RegistrationRequestSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        raw_token = data["token"]
        hashed = tokens.hash_token(raw_token)
        try:
            reg = (
                RunnerRegistrationToken.objects.select_for_update()
                .get(token_hash=hashed)
            )
        except RunnerRegistrationToken.DoesNotExist:
            return Response(
                {"error": "invalid or expired registration token"},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        if not reg.is_valid():
            return Response(
                {"error": "registration token already used or expired"},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        if not can_register_another(reg.created_by_id, reg.workspace_id):
            return Response(
                {"error": f"runner cap reached ({Runner.MAX_PER_USER})"},
                status=status.HTTP_409_CONFLICT,
            )

        minted = tokens.mint_runner_secret()
        runner = Runner.objects.create(
            owner=reg.created_by,
            workspace=reg.workspace,
            name=data["runner_name"][:128],
            credential_hash=minted.hashed,
            credential_fingerprint=minted.fingerprint,
            os=data["os"][:32],
            arch=data["arch"][:32],
            runner_version=data["version"][:32],
            protocol_version=data["protocol_version"],
        )
        reg.consumed_at = timezone.now()
        reg.consumed_by_runner = runner
        reg.save(update_fields=["consumed_at", "consumed_by_runner"])

        payload = RegistrationResponseSerializer(
            {
                "runner_id": runner.id,
                "runner_secret": minted.raw,
                "heartbeat_interval_secs": HEARTBEAT_INTERVAL_SECS,
                "protocol_version": PROTOCOL_VERSION,
            }
        ).data
        return Response(payload, status=status.HTTP_201_CREATED)


class RunnerDeregisterEndpoint(APIView):
    """POST /api/v1/runner/<uuid>/deregister/

    Called by the daemon during ``pidash remove``. Authenticated
    with the runner's own bearer secret; the server marks the runner revoked.
    """

    authentication_classes = [RunnerBearerAuthentication]
    permission_classes = []
    # DRF's default throttles call ``request.user.is_authenticated``; our
    # bearer auth puts a ``Runner`` instance there, so skip the throttle chain.
    throttle_classes: list = []

    def post(self, request, runner_id):
        runner = getattr(request, "auth_runner", None)
        if runner is None or str(runner.id) != str(runner_id):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        runner.revoke()
        close_runner_session(runner.pk)
        return Response({"ok": True})


class RunnerRotateEndpoint(APIView):
    """POST /api/v1/runner/<uuid>/rotate/

    Runner authenticates with its current bearer secret and receives a new
    one. The old credential is immediately invalidated. The daemon writes the
    new secret to ``credentials.toml`` and reconnects.
    """

    authentication_classes = [RunnerBearerAuthentication]
    permission_classes = []
    throttle_classes: list = []

    def post(self, request, runner_id):
        runner = getattr(request, "auth_runner", None)
        if runner is None or str(runner.id) != str(runner_id):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        minted = tokens.mint_runner_secret()
        # Update in a single statement so no window has two valid credentials.
        Runner.objects.filter(pk=runner.pk).update(
            credential_hash=minted.hashed,
            credential_fingerprint=minted.fingerprint,
        )
        # Any WebSocket authenticated with the old secret is now orphaned —
        # force-close it so the daemon reconnects with the new credential.
        close_runner_session(runner.pk)
        return Response(
            {
                "runner_id": str(runner.id),
                "runner_secret": minted.raw,
                "heartbeat_interval_secs": HEARTBEAT_INTERVAL_SECS,
                "protocol_version": PROTOCOL_VERSION,
            }
        )


class RegistrationTokenCreateEndpoint(APIView):
    """POST /api/runners/tokens/  — web UI mints a one-time registration code."""

    permission_classes = [IsAuthenticated]

    def post(self, request):
        workspace_id = request.data.get("workspace")
        label = (request.data.get("label") or "")[:128]
        if not workspace_id:
            return Response(
                {"error": "workspace is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not can_register_another(request.user.id, workspace_id):
            return Response(
                {"error": f"runner cap reached ({Runner.MAX_PER_USER})"},
                status=status.HTTP_409_CONFLICT,
            )
        minted = tokens.mint_registration_token()
        record = RunnerRegistrationToken.objects.create(
            workspace_id=workspace_id,
            created_by=request.user,
            token_hash=minted.hashed,
            label=label,
            expires_at=minted.expires_at,
        )
        return Response(
            {
                "registration": RegistrationTokenSerializer(record).data,
                "token": minted.raw,
            },
            status=status.HTTP_201_CREATED,
        )

    def get(self, request):
        qs = RunnerRegistrationToken.objects.filter(created_by=request.user).order_by(
            "-created_at"
        )
        return Response(RegistrationTokenSerializer(qs, many=True).data)
