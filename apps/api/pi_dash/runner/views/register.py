# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import IntegrityError, transaction
from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import AllowAny, IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.db.models import APIToken
from pi_dash.runner.authentication import RunnerBearerAuthentication
from pi_dash.runner.models import (
    MachineToken,
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
from pi_dash.runner.services.pubsub import close_runner_session, send_to_runner


HEARTBEAT_INTERVAL_SECS = 25
PROTOCOL_VERSION = 2


class HealthEndpoint(APIView):
    authentication_classes: list = []
    permission_classes = [AllowAny]

    def get(self, request):
        return Response({"ok": True, "protocol_version": PROTOCOL_VERSION})


class RegisterEndpoint(APIView):
    """POST /api/v1/runner/register/ — one-time-token to runner-secret exchange.

    Called by the daemon during ``pidash configure``. Issues two
    independent credentials in a single response:

    - ``runner_secret`` — long-lived bearer for the daemon's WS connection.
    - ``api_token``    — ``X-Api-Key`` for the public REST API at
      ``/api/v1/`` so the same install can drive work-item CRUD without
      re-authenticating. Tied to ``reg.created_by`` and revocable
      independently of the runner.
    """

    authentication_classes: list = []
    permission_classes = [AllowAny]

    @transaction.atomic
    def post(self, request):
        from pi_dash.db.models.project import Project
        from pi_dash.runner.models import Pod

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

        # Resolve the project + pod for this runner. ``project`` is
        # required: the runner is bound to one project for its
        # lifetime. ``pod`` is optional and defaults to the project's
        # default pod.
        project_identifier = (request.data.get("project") or "").strip()
        if not project_identifier:
            return Response(
                {"error": "project is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        project = Project.objects.filter(
            workspace=reg.workspace, identifier=project_identifier
        ).first()
        if project is None:
            return Response(
                {"error": "project not found in registration's workspace"},
                status=status.HTTP_404_NOT_FOUND,
            )

        pod_name = (request.data.get("pod") or "").strip()
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

        minted = tokens.mint_runner_secret()
        try:
            runner = Runner.objects.create(
                owner=reg.created_by,
                workspace=reg.workspace,
                pod=pod,
                name=data["runner_name"],
                credential_hash=minted.hashed,
                credential_fingerprint=minted.fingerprint,
                os=data["os"][:32],
                arch=data["arch"][:32],
                runner_version=data["version"][:32],
                protocol_version=data["protocol_version"],
            )
        except IntegrityError:
            # `UNIQUE(workspace_id, name)` violation. The registration token
            # stays unconsumed because the `reg.consumed_at` / `reg.save(...)`
            # writes are below this return path — they simply never execute.
            # (Returning a Response does not by itself roll back the atomic
            # block; it commits. The Runner.create() failed so no row landed,
            # and the token-consume writes haven't been issued yet, so the
            # end state is "nothing changed" regardless.) The runner retries
            # auto-generated names transparently; a user-supplied `--name`
            # collision surfaces as a loud error client-side.
            return Response(
                {"error": "runner_name_taken"},
                status=status.HTTP_409_CONFLICT,
            )
        reg.consumed_at = timezone.now()
        reg.consumed_by_runner = runner
        reg.save(update_fields=["consumed_at", "consumed_by_runner"])

        # Mint a CLI API token alongside the runner secret. Different
        # threat model than the runner secret (interactive user actions
        # vs. background daemon), so kept as a separate row that can be
        # revoked independently in the user's API tokens UI.
        api_token = APIToken.objects.create(
            user=reg.created_by,
            user_type=1 if reg.created_by.is_bot else 0,
            workspace=reg.workspace,
            label=f"runner: {data['runner_name'][:96]}",
            description="Auto-issued at runner enrollment for the pidash CLI.",
            # Route CLI traffic through the 300/min ServiceTokenRateThrottle
            # instead of the default 60/min user-key throttle — a single turn
            # can easily fan out to dozens of GET/PATCH calls.
            is_service=True,
        )

        payload = RegistrationResponseSerializer(
            {
                "runner_id": runner.id,
                "runner_secret": minted.raw,
                "workspace_slug": reg.workspace.slug,
                "api_token": api_token.token,
                "heartbeat_interval_secs": HEARTBEAT_INTERVAL_SECS,
                "protocol_version": PROTOCOL_VERSION,
            }
        ).data
        # `pod_id` is not on the serializer (would need a schema bump to
        # add). Append it directly so the daemon can stamp it into
        # config.toml's [[runner]] block. Daemons that ignore the field
        # keep working.
        payload["pod_id"] = str(pod.id)
        payload["project_identifier"] = project.identifier
        return Response(payload, status=status.HTTP_201_CREATED)


class RunnerDeregisterEndpoint(APIView):
    """POST /api/v1/runner/<uuid>/deregister/

    Called by the daemon during ``pidash remove`` and ``pidash token
    remove-runner``. Authenticated with the runner's own bearer secret
    (legacy single-runner) or with ``X-Token-Id`` + the token's bearer
    (token / multi-runner). The server marks the runner revoked.

    Connection teardown semantics differ by auth mode:

    - **Legacy** — close the WebSocket. One connection ↔ one runner, so
      there's nothing else on the socket worth preserving.
    - **Token mode** — emit ``ServerMsg::RemoveRunner`` so the daemon
      tears down ONLY this runner's ``RunnerLoop`` while the shared WS
      and the other runners under this token stay up. Force-closing
      the connection here would knock every sibling runner offline as
      a side effect.
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
        in_token_mode = runner.machine_token_id is not None
        runner.revoke()
        if in_token_mode:
            send_to_runner(
                runner.pk,
                {
                    "type": "remove_runner",
                    "runner_id": str(runner.id),
                    "reason": "deregistered",
                },
            )
        else:
            close_runner_session(runner.pk)
        return Response({"ok": True})


class RunnerLinkToTokenEndpoint(APIView):
    """POST /api/v1/runner/<uuid>/link-to-token/

    Migrate a runner registered via the legacy ``/register/`` flow onto a
    MachineToken so its daemon can switch to token-auth without
    re-registering. Called by ``pidash token install`` after the user
    creates a token in the cloud UI.

    Auth: the runner's existing ``runner_secret`` (legacy bearer). At
    this point the daemon has NOT yet written the ``[token]`` block to
    ``credentials.toml``, so this is the only credential it can present.
    The body carries the new token's id + secret, which we verify
    server-side before writing the FK.

    Body:
        { "token_id": "<UUID>", "token_secret": "<RAW>" }

    Failure modes (all 4xx, runner stays unlinked, client should not
    write the [token] block):
    - 400 if token_id isn't a UUID
    - 401 if token_secret doesn't match a non-revoked token
    - 400 if the token belongs to a different workspace than the runner
    - 409 if the runner is revoked, or already linked to a different
      non-revoked token (operator must revoke the old link first; we
      refuse silent re-binding because it would let a compromised
      token's owner steal a runner from another machine)
    """

    authentication_classes = [RunnerBearerAuthentication]
    permission_classes = []
    throttle_classes: list = []

    def post(self, request, runner_id):
        import uuid as _uuid

        runner = getattr(request, "auth_runner", None)
        if runner is None or str(runner.id) != str(runner_id):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        if runner.status == RunnerStatus.REVOKED:
            return Response(
                {"error": "runner is revoked"}, status=status.HTTP_409_CONFLICT
            )
        token_id_raw = request.data.get("token_id") or ""
        token_secret_raw = request.data.get("token_secret") or ""
        if not token_id_raw or not token_secret_raw:
            return Response(
                {"error": "token_id and token_secret are required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        try:
            token_id = _uuid.UUID(str(token_id_raw))
        except (ValueError, AttributeError):
            return Response(
                {"error": "invalid token_id"}, status=status.HTTP_400_BAD_REQUEST
            )
        secret_hash = tokens.hash_token(str(token_secret_raw))
        token = MachineToken.objects.filter(
            id=token_id, secret_hash=secret_hash, revoked_at__isnull=True
        ).first()
        if token is None:
            return Response(
                {"error": "invalid or revoked token"},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        if token.workspace_id != runner.workspace_id:
            return Response(
                {"error": "token belongs to a different workspace"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if (
            runner.machine_token_id is not None
            and runner.machine_token_id != token.id
            and MachineToken.objects.filter(
                id=runner.machine_token_id, revoked_at__isnull=True
            ).exists()
        ):
            return Response(
                {"error": "runner already linked to a different active token"},
                status=status.HTTP_409_CONFLICT,
            )
        Runner.objects.filter(pk=runner.pk).update(machine_token=token)
        return Response({"ok": True, "token_id": str(token.id)})


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

    authentication_classes = [BaseSessionAuthentication]
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
