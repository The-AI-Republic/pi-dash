# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Cloud-driven runner creation on a connected dev machine.

The web "Add runner" modal can target a dev machine that has an active
machine control session (see :mod:`pi_dash.runner.views.machine_sessions`)
instead of asking the operator to copy a ``pidash runner add`` command.
Three endpoints implement the loop:

- ``POST /api/runners/dev-machines/<mid>/create-runner/`` (web session
  auth) — validates and enqueues a ``create_runner`` control message on
  the machine outbox. Fails fast with 409 when the machine is offline
  (``create_runner`` is on the outbox's offline-reject list).
- ``GET /api/runners/dev-machines/<mid>/create-runner/<request_id>/``
  (web session auth) — read the daemon-reported result so the modal can
  poll to completion.
- ``POST /api/v1/runner/dev-machines/<mid>/commands/<request_id>/result/``
  (machine token auth) — the daemon writes back ``ok`` / ``error`` after
  executing the command locally.
"""

from __future__ import annotations

import logging
import uuid as _uuid

from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.authentication import MachineTokenAuthentication
from pi_dash.runner.models import DevMachine
from pi_dash.runner.services import machine_outbox
from pi_dash.runner.services.machine_outbox import MachineOfflineError
from pi_dash.runner.services.permissions import is_workspace_member
from pi_dash.runner.services.pubsub import send_to_machine
from pi_dash.runner.views.enrollment import _RUNNER_NAME_RE
from pi_dash.runner.views.machine_sessions import _auth_dev_machine
from pi_dash.runner.views.runners import (
    _machine_is_in_workspace_scope,
    _request_workspace_id,
)

logger = logging.getLogger(__name__)

# Mirrors runner/src/config/schema.rs:AgentKind (kebab-case wire values)
# and the web modal's AGENT_OPTIONS.
_VALID_AGENTS = frozenset({"claude-code", "codex", "cursor-agent", "open-claw"})

_RESULT_STATUSES = frozenset({"ok", "error"})


def _scoped_machine(request, machine_id, workspace_id):
    """Resolve the target machine under the caller's workspace scope.

    Returns ``(machine, error_response)`` — exactly one is non-None.
    """
    machine = DevMachine.objects.filter(pk=machine_id).first()
    if machine is None or not _machine_is_in_workspace_scope(request.user, machine, workspace_id):
        return None, Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
    if machine.revoked_at is not None:
        return None, Response({"error": "dev_machine_revoked"}, status=status.HTTP_409_CONFLICT)
    return machine, None


class MachineCreateRunnerEndpoint(APIView):
    """``POST /dev-machines/<mid>/create-runner/`` — enqueue remote creation."""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, machine_id):
        from pi_dash.db.models.project import Project
        from pi_dash.db.models.workspace import Workspace

        workspace_id = _request_workspace_id(request)
        if not workspace_id:
            return Response(
                {"error": "workspace is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not is_workspace_member(request.user, workspace_id):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        machine, err = _scoped_machine(request, machine_id, workspace_id)
        if err is not None:
            return err

        project_identifier = (request.data.get("project") or "").strip()
        if not project_identifier:
            return Response(
                {"error": "project is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        workspace = Workspace.objects.filter(pk=workspace_id).first()
        if workspace is None:
            return Response({"error": "workspace_not_found"}, status=status.HTTP_404_NOT_FOUND)
        if not Project.objects.filter(workspace_id=workspace_id, identifier=project_identifier).exists():
            return Response({"error": "project_not_found"}, status=status.HTTP_404_NOT_FOUND)

        name = (request.data.get("name") or "").strip()[:128]
        if name and not _RUNNER_NAME_RE.match(name):
            return Response(
                {
                    "error": "invalid_runner_name",
                    "error_description": (
                        "name must start with a letter, digit, or underscore "
                        "and contain only letters, digits, underscore, dot, or dash"
                    ),
                },
                status=status.HTTP_400_BAD_REQUEST,
            )

        agent = (request.data.get("agent") or "").strip() or "claude-code"
        if agent not in _VALID_AGENTS:
            return Response({"error": "invalid_agent"}, status=status.HTTP_400_BAD_REQUEST)

        request_id = str(_uuid.uuid4())
        message = {
            "type": "create_runner",
            "request_id": request_id,
            "workspace_slug": workspace.slug,
            "project": project_identifier,
            "pod": (request.data.get("pod") or "").strip(),
            "name": name,
            "working_dir": (request.data.get("working_dir") or "").strip(),
            "agent": agent,
            "model": (request.data.get("model") or "").strip(),
            "reasoning_effort": (request.data.get("reasoning_effort") or "").strip(),
        }

        # Pending marker first: if the daemon executes fast, its result
        # write must land on an existing key semantics-wise (overwrites
        # are fine either way, but the marker also lets the status
        # endpoint distinguish "in flight" from "unknown request").
        machine_outbox.set_command_result(
            request_id,
            {"status": "pending", "requested_at": timezone.now().isoformat()},
        )
        try:
            send_to_machine(machine.id, message)
        except MachineOfflineError:
            machine_outbox.set_command_result(
                request_id,
                {"status": "error", "error": "machine_offline"},
            )
            return Response(
                {"error": "machine_offline"},
                status=status.HTTP_409_CONFLICT,
            )

        return Response({"request_id": request_id}, status=status.HTTP_202_ACCEPTED)


class MachineCreateRunnerStatusEndpoint(APIView):
    """``GET /dev-machines/<mid>/create-runner/<request_id>/`` — poll result."""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request, machine_id, request_id):
        workspace_id = _request_workspace_id(request)
        if not workspace_id:
            return Response(
                {"error": "workspace is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not is_workspace_member(request.user, workspace_id):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        machine, err = _scoped_machine(request, machine_id, workspace_id)
        if err is not None:
            return err

        result = machine_outbox.get_command_result(request_id)
        if result is None:
            return Response({"error": "unknown_request"}, status=status.HTTP_404_NOT_FOUND)
        return Response({"request_id": str(request_id), **result})


class MachineCommandResultEndpoint(APIView):
    """``POST /dev-machines/<mid>/commands/<request_id>/result/``.

    Daemon-side write-back, authenticated by the machine-bound ``mt_``
    token exactly like the machine session endpoints.
    """

    authentication_classes = [MachineTokenAuthentication]
    permission_classes: list = []
    throttle_classes: list = []

    def post(self, request, dev_machine_id, request_id):
        machine = _auth_dev_machine(request, dev_machine_id)
        if machine is None:
            return Response(
                {"error": "dev_machine_mismatch"},
                status=status.HTTP_403_FORBIDDEN,
            )

        result_status = str(request.data.get("status") or "")
        if result_status not in _RESULT_STATUSES:
            return Response({"error": "invalid_status"}, status=status.HTTP_400_BAD_REQUEST)

        payload = {
            "status": result_status,
            "runner_id": str(request.data.get("runner_id") or ""),
            "runner_name": str(request.data.get("runner_name") or "")[:128],
            "error": str(request.data.get("error") or "")[:512],
            "reported_at": timezone.now().isoformat(),
        }
        machine_outbox.set_command_result(request_id, payload)
        return Response(status=status.HTTP_204_NO_CONTENT)
