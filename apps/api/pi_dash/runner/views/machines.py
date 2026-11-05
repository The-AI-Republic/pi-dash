# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Machine identity endpoints (built_engine_as_runner §4.10, §11, §13).

Daemon-facing (``/api/v1/runner/``):

- ``PATCH dev-machines/<id>/`` — machine token of that machine. The
  daemon's engine pause (``pidash engine pause``) and rename.
- ``GET dev-machines/<id>/replace-candidates/`` — machine token of that
  machine, or the owner's session. The "Is this <label>? Replace it"
  prompt after a sign-in that created a new machine.
- ``POST dev-machines/<source-id>/merge-into/`` — the owner's session.
  Merge a replaced or legacy machine into the one now serving the computer.

Web (``/api/runners/``):

- ``POST dev-machines/<id>/engine-sign-out/`` — "Sign out the engine" on the
  machine page (owner only).
"""

from __future__ import annotations

import uuid as _uuid

from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.authentication import MachineTokenAuthentication
from pi_dash.runner.models import DevMachine, EngineCredentialRevokeReason
from pi_dash.runner.serializers import DevMachineSerializer
from pi_dash.runner.services.engine_credentials import revoke_engine_credential
from pi_dash.runner.services.machine_identity import (
    MachineControlError,
    MergeError,
    merge_dev_machine,
    parse_machine_controls,
    replace_candidates,
    update_machine_controls,
)


class DaemonDevMachineEndpoint(APIView):
    """``PATCH /api/v1/runner/dev-machines/<id>/`` with the machine's own
    token: ``engine_paused_by_owner`` (bool) and/or ``label`` (string)."""

    authentication_classes = [MachineTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    FIELDS = ("engine_paused_by_owner", "label")

    def patch(self, request, dev_machine_id):
        token = getattr(request, "auth_machine_token", None)
        if token is None or token.dev_machine_id is None or str(token.dev_machine_id) != str(dev_machine_id):
            return Response({"error": "dev_machine_mismatch"}, status=status.HTTP_403_FORBIDDEN)
        try:
            values = parse_machine_controls(request.data, self.FIELDS)
            machine = update_machine_controls(dev_machine_id, token.user, values)
        except MachineControlError as exc:
            return Response({"error": exc.code}, status=exc.http_status)
        return Response(DevMachineSerializer(machine).data, status=status.HTTP_200_OK)


def _candidate_payload(machine: DevMachine) -> dict:
    return {
        "id": str(machine.pk),
        "label": machine.label,
        "host_label": machine.host_label,
        "display_name": machine.display_name,
        "last_seen_at": machine.last_seen_at.isoformat() if machine.last_seen_at else None,
        "created_at": machine.created_at.isoformat() if machine.created_at else None,
        "provisioning": machine.provisioning,
        "live_runner_count": machine.runners.filter(revoked_at__isnull=True).count(),
    }


class DevMachineReplaceCandidatesEndpoint(APIView):
    """``GET dev-machines/<id>/replace-candidates/`` — the caller's offline
    machines with the same ``host_label`` and no live session."""

    authentication_classes = [MachineTokenAuthentication, BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]
    throttle_classes: list = []

    def get(self, request, dev_machine_id):
        token = getattr(request, "auth_machine_token", None)
        if token is not None and str(token.dev_machine_id) != str(dev_machine_id):
            return Response({"error": "dev_machine_mismatch"}, status=status.HTTP_403_FORBIDDEN)
        machine = DevMachine.objects.filter(pk=dev_machine_id, owner=request.user).first()
        if machine is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        return Response({"candidates": [_candidate_payload(m) for m in replace_candidates(machine)]})


class DevMachineMergeEndpoint(APIView):
    """``POST dev-machines/<source-id>/merge-into/`` with
    ``{"target_dev_machine_id": "<uuid>"}`` (``target`` accepted too)."""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, dev_machine_id):
        target_id = str(request.data.get("target_dev_machine_id") or request.data.get("target") or "").strip()
        if not target_id:
            return Response({"error": "target_dev_machine_id is required"}, status=status.HTTP_400_BAD_REQUEST)
        try:
            target_id = str(_uuid.UUID(target_id))
        except ValueError:
            return Response({"error": "invalid_target_dev_machine_id"}, status=status.HTTP_400_BAD_REQUEST)
        try:
            result = merge_dev_machine(user=request.user, source_id=dev_machine_id, target_id=target_id)
        except MergeError as exc:
            return Response({"error": exc.code}, status=exc.http_status)
        return Response(result, status=status.HTTP_200_OK)


class DevMachineEngineSignOutEndpoint(APIView):
    """``POST /api/runners/dev-machines/<id>/engine-sign-out/`` — revoke the
    machine's engine credential only (state ``signed_out``)."""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, machine_id):
        from pi_dash.runner.views.runners import (
            _machine_is_in_workspace_scope,
            _request_workspace_id,
            _serialize_dev_machine,
        )

        machine = DevMachine.objects.filter(pk=machine_id, owner=request.user).first()
        if machine is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        # ``{workspace}`` (the machine page's scope) is optional; when given,
        # the machine must be in it, like every other machine-page action.
        workspace_id = _request_workspace_id(request)
        if workspace_id and not _machine_is_in_workspace_scope(request.user, machine, workspace_id):
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        revoke_engine_credential(machine, reason=EngineCredentialRevokeReason.SIGNED_OUT)
        machine.refresh_from_db()
        if workspace_id:
            return Response(_serialize_dev_machine(machine, request.user, workspace_id), status=status.HTTP_200_OK)
        return Response(DevMachineSerializer(machine).data, status=status.HTTP_200_OK)
