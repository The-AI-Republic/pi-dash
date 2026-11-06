# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Machine identity endpoints (built_engine_as_runner §4.10, §11, §13).

Daemon-facing (``/api/v1/runner/``):

- ``GET dev-machines/<id>/`` — machine token of that machine (any of its
  workspace tokens). The daemon's view of its machine: engine pause and
  sign-in fields plus the machine's runner rows in the token's workspace,
  revoked rows included, so it can reconcile its runner blocks (§5).
- ``PATCH dev-machines/<id>/`` — machine token of that machine. The
  daemon's engine pause (``pidash engine pause``) and rename; answers the
  same body as ``GET``.
- ``GET dev-machines/<id>/replace-candidates/`` — machine token of that
  machine, or the owner's session. The "Is this <label>? Replace it"
  prompt after a sign-in that created a new machine.
- ``POST dev-machines/<source-id>/merge-into/`` — the owner's session, or
  the machine token of the *target* (the machine now serving the computer,
  so ``pidash auth login`` can offer "Replace it" too). Merge a replaced or
  legacy machine into that one.

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
from pi_dash.runner.models import DevMachine, EngineCredentialRevokeReason, Runner, RunnerProvisioning
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


_AGENT_CAPABILITY_PREFIX = "agent:"
ENGINE_AGENT_KIND = "engine"


def _runner_agent_kind(runner: Runner) -> str:
    """``engine`` for a built-in engine runner, else the ``agent:<kind>``
    capability the daemon reported at session open (empty if none yet)."""
    if runner.provisioning == RunnerProvisioning.DESKTOP_BUNDLED:
        return ENGINE_AGENT_KIND
    capabilities = runner.capabilities if isinstance(runner.capabilities, list) else []
    for capability in capabilities:
        if isinstance(capability, str) and capability.startswith(_AGENT_CAPABILITY_PREFIX):
            return capability[len(_AGENT_CAPABILITY_PREFIX) :]
    return ""


def _daemon_runner_row(runner: Runner) -> dict:
    metadata = runner.dev_metadata if isinstance(runner.dev_metadata, dict) else {}
    pod = runner.pod
    project = pod.project if pod is not None else None
    return {
        "id": str(runner.pk),
        "name": runner.name,
        "workspace_slug": runner.workspace.slug,
        "project_id": str(project.pk) if project is not None else None,
        "project_identifier": project.identifier if project is not None else "",
        "pod_id": str(pod.pk) if pod is not None else None,
        "agent_kind": _runner_agent_kind(runner),
        "provisioning": runner.provisioning,
        "status": runner.status,
        "revoked_at": runner.revoked_at.isoformat() if runner.revoked_at else None,
        "revoked_reason": runner.revoked_reason,
        # As last reported by the daemon at session open; empty when never.
        "working_dir": metadata.get("working_dir", ""),
    }


def daemon_machine_payload(machine: DevMachine, workspace_id) -> dict:
    """The machine as its own daemon sees it (``GET``/``PATCH``
    ``dev-machines/<id>/``): the machine fields, including the engine pause
    (``engine_paused_by_owner``, ``engine_auto_pause_reason``) and sign-in
    (``engine_sign_in_state``, ``engine_credential_expires_at``), plus
    ``runners``: the machine's runner rows in the calling token's workspace
    (the daemon reconciles per workspace, one read per workspace token),
    revoked rows included, oldest first."""
    runners = (
        Runner.objects.filter(dev_machine=machine, workspace_id=workspace_id)
        .select_related("workspace", "pod", "pod__project")
        .order_by("created_at", "id")
    )
    return {**DevMachineSerializer(machine).data, "runners": [_daemon_runner_row(r) for r in runners]}


class DaemonDevMachineEndpoint(APIView):
    """``/api/v1/runner/dev-machines/<id>/`` with the machine's own token.

    ``GET`` answers :func:`daemon_machine_payload`. ``PATCH`` takes
    ``engine_paused_by_owner`` (bool) and/or ``label`` (string) and answers
    the same body. Any of the machine's workspace tokens works, and the
    ``runners`` list is scoped to that token's workspace; a token of
    another machine, or one not bound to a machine, is refused with 403.
    """

    authentication_classes = [MachineTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    FIELDS = ("engine_paused_by_owner", "label")

    @staticmethod
    def _own_machine_token(request, dev_machine_id):
        token = getattr(request, "auth_machine_token", None)
        if token is None or token.dev_machine_id is None or str(token.dev_machine_id) != str(dev_machine_id):
            return None
        return token

    def get(self, request, dev_machine_id):
        token = self._own_machine_token(request, dev_machine_id)
        if token is None:
            return Response({"error": "dev_machine_mismatch"}, status=status.HTTP_403_FORBIDDEN)
        machine = DevMachine.objects.filter(pk=dev_machine_id, owner_id=token.user_id).first()
        if machine is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        return Response(daemon_machine_payload(machine, token.workspace_id), status=status.HTTP_200_OK)

    def patch(self, request, dev_machine_id):
        token = self._own_machine_token(request, dev_machine_id)
        if token is None:
            return Response({"error": "dev_machine_mismatch"}, status=status.HTTP_403_FORBIDDEN)
        try:
            values = parse_machine_controls(request.data, self.FIELDS)
            machine = update_machine_controls(dev_machine_id, token.user, values)
        except MachineControlError as exc:
            return Response({"error": exc.code}, status=exc.http_status)
        return Response(daemon_machine_payload(machine, token.workspace_id), status=status.HTTP_200_OK)


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


def _token_may_merge(user, source_id, target_id) -> bool:
    target = DevMachine.objects.filter(pk=target_id, owner=user).first()
    source = DevMachine.objects.filter(pk=source_id, owner=user).first()
    if target is None or source is None:
        # Let the merge answer 404 the same way for every caller.
        return True
    if source.merged_into_id is not None and str(source.merged_into_id) == str(target.pk):
        return True
    # Liveness is left to the merge, which answers
    # ``source_machine_has_live_session`` in plain words.
    return bool(target.host_label) and source.host_label == target.host_label


class DevMachineMergeEndpoint(APIView):
    """``POST dev-machines/<source-id>/merge-into/`` with
    ``{"target_dev_machine_id": "<uuid>"}`` (``target`` accepted too)."""

    authentication_classes = [MachineTokenAuthentication, BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]
    throttle_classes: list = []

    def post(self, request, dev_machine_id):
        target_id = str(request.data.get("target_dev_machine_id") or request.data.get("target") or "").strip()
        if not target_id:
            return Response({"error": "target_dev_machine_id is required"}, status=status.HTTP_400_BAD_REQUEST)
        try:
            target_id = str(_uuid.UUID(target_id))
        except ValueError:
            return Response({"error": "invalid_target_dev_machine_id"}, status=status.HTTP_400_BAD_REQUEST)
        # A machine token may only merge into its own machine: the computer
        # it serves is the one that replaces the old machine (§4.10). The
        # old machine's token, or a third machine's, is refused.
        token = getattr(request, "auth_machine_token", None)
        if token is not None and str(token.dev_machine_id) != target_id:
            return Response({"error": "dev_machine_mismatch"}, status=status.HTTP_403_FORBIDDEN)
        # And only a machine the "Replace it" prompt can offer: the same
        # owner's machine with the same host_label. Any other merge (a
        # legacy desktop machine, another hostname) needs the owner's
        # session, so a leaked machine token cannot pull in every offline
        # machine of the account. A retry of a merge already done passes.
        if token is not None and not _token_may_merge(request.user, dev_machine_id, target_id):
            return Response({"error": "not_a_replace_candidate"}, status=status.HTTP_403_FORBIDDEN)
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
