# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Machine enrollment for the Pi Dash desktop app.

``pidash auth login`` walks a user through a device-code flow in a terminal to
obtain a machine token. The desktop app already holds an authenticated session,
so making it replay that dance would be theatre — this endpoint is the
session-cookie equivalent: it mints the same ``MachineToken``, against a
``DevMachine`` marked as Pi Dash-provisioned, and hands it back once.

Everything downstream is unchanged: the bundled daemon writes that token to
``[cli].token`` exactly as a hand-installed runner does, and registers runners
through the ordinary machine-token path.
"""

from __future__ import annotations

import logging

from django.conf import settings
from django.db import transaction
from django.db.models import Q
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.managed_runner.permissions import IsDesktopSession
from pi_dash.runner.models import DevMachine, MachineToken, Runner, RunnerProvisioning, RunnerStatus
from pi_dash.runner.services import tokens
from pi_dash.runner.services.dev_machines import (
    InvalidMachineFingerprint,
    parse_machine_fingerprint,
    resolve_dev_machine,
)
from pi_dash.runner.services.permissions import is_workspace_member

logger = logging.getLogger(__name__)

_DESKTOP_TOKEN_LABEL_PREFIX = "desktop: "


def _version_is_allowed(reported: str) -> bool:
    """Whether ``reported`` desktop version satisfies the configured floor.

    Compared as dotted integer tuples so ``0.10.0`` sorts above ``0.9.0``. An
    unparseable or absent version is allowed: refusing on a malformed header
    would lock users out on a client bug, and the floor exists to force
    upgrades past known-broken builds, not to authenticate.
    """
    floor = (settings.DESKTOP_MIN_VERSION_FOR_MANAGED_RUNNER or "").strip()
    if not floor:
        return True

    def parse(value: str):
        parts = []
        for chunk in (value or "").strip().split("."):
            digits = "".join(ch for ch in chunk if ch.isdigit())
            if not digits:
                return None
            parts.append(int(digits))
        return tuple(parts) or None

    wanted = parse(floor)
    got = parse(reported)
    if wanted is None or got is None:
        return True
    return got >= wanted


class DesktopEnrollEndpoint(APIView):
    """``POST``/``DELETE /api/v1/runner/dev-machines/desktop-enroll/``.

    ``POST`` resolves the computer's one ``DevMachine`` (by
    ``machine_fingerprint`` when sent) and rotates the workspace's machine
    token onto it rather than accumulating rows, which is what makes "sign
    out, sign back in" cheap. Kept for older desktop builds; current ones
    drive the shared ``pidash`` daemon instead.

    ``DELETE`` revokes the token and marks the machine's bundled runners
    offline; the rows survive so the next sign-in on this machine reuses them
    instead of burning through the per-project cap.
    """

    permission_classes = [IsDesktopSession]
    throttle_classes: list = []

    def post(self, request):
        from pi_dash.db.models.workspace import Workspace

        workspace_slug = (request.data.get("workspace_slug") or "").strip()
        host_label = (request.data.get("host_label") or "").strip()[:255]
        app_version = (request.data.get("app_version") or "").strip()[:32]
        try:
            fingerprint = parse_machine_fingerprint(request.data.get("machine_fingerprint"))
        except InvalidMachineFingerprint:
            return Response({"error": "invalid_machine_fingerprint"}, status=status.HTTP_400_BAD_REQUEST)

        if not workspace_slug:
            return Response({"error": "workspace_slug is required"}, status=status.HTTP_400_BAD_REQUEST)
        if not host_label:
            return Response({"error": "host_label is required"}, status=status.HTTP_400_BAD_REQUEST)
        if not _version_is_allowed(app_version):
            return Response(
                {
                    "error": "desktop_update_required",
                    "error_description": (
                        f"Pi Dash Desktop {settings.DESKTOP_MIN_VERSION_FOR_MANAGED_RUNNER} or newer is required."
                    ),
                },
                status=status.HTTP_409_CONFLICT,
            )

        workspace = Workspace.objects.filter(slug=workspace_slug).first()
        # Same 404 for "no such workspace" and "not yours" — don't leak which.
        if workspace is None or not is_workspace_member(request.user, workspace.id):
            return Response({"error": "workspace_not_found"}, status=status.HTTP_404_NOT_FOUND)

        with transaction.atomic():
            # The shared resolution rule: one record per computer, keyed on
            # the fingerprint. Builds that send none fall back to the
            # owner's desktop-provisioned record with the same host label.
            dev_machine = resolve_dev_machine(
                user=request.user,
                dev_machine_id=None,
                host_label=host_label,
                fingerprint=fingerprint,
                provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
            )

            # One live token per (machine, workspace): rotating rather than
            # appending means a lost laptop is revoked by one row, not N.
            MachineToken.objects.select_for_update().filter(
                dev_machine=dev_machine,
                workspace=workspace,
                revoked_at__isnull=True,
            ).update(revoked_at=timezone.now())
            minted = tokens.mint_machine_token()
            MachineToken.objects.create(
                user=request.user,
                dev_machine=dev_machine,
                workspace=workspace,
                host_label=host_label,
                token_hash=minted.hashed,
                token_fingerprint=minted.fingerprint,
                label=f"{_DESKTOP_TOKEN_LABEL_PREFIX}{host_label[:88]}",
                is_service=True,
            )

        logger.info(
            "managed_runner.enrolled user=%s dev_machine=%s workspace=%s",
            request.user.id,
            dev_machine.id,
            workspace.id,
        )
        return Response(
            {
                "dev_machine_id": str(dev_machine.id),
                "machine_token": minted.raw,
                "workspace_slug": workspace.slug,
                "managed_runner_enabled": bool(settings.MANAGED_RUNNER_ENABLED),
                "graceful_stop_seconds": settings.MANAGED_RUNNER_GRACEFUL_STOP_SECS,
            },
            status=status.HTTP_201_CREATED,
        )

    def delete(self, request):
        host_label = (request.data.get("host_label") or request.query_params.get("host_label") or "").strip()[:255]
        # Desktop enrollment may now resolve to the computer's shared record
        # (also used by a hand-installed CLI), so only the tokens this
        # endpoint minted (``desktop:`` label) are revoked, never the CLI's.
        desktop_tokens = MachineToken.objects.filter(
            user=request.user,
            label__startswith=_DESKTOP_TOKEN_LABEL_PREFIX,
            revoked_at__isnull=True,
            dev_machine__isnull=False,
        )
        machines = DevMachine.objects.filter(
            owner=request.user,
            revoked_at__isnull=True,
            merged_into__isnull=True,
        ).filter(
            Q(provisioning=RunnerProvisioning.DESKTOP_BUNDLED)
            | Q(id__in=desktop_tokens.values_list("dev_machine_id", flat=True))
        )
        if host_label:
            machines = machines.filter(host_label=host_label)

        machine_ids = list(machines.values_list("id", flat=True))
        if not machine_ids:
            return Response(status=status.HTTP_204_NO_CONTENT)

        with transaction.atomic():
            desktop_tokens.filter(dev_machine_id__in=machine_ids).update(revoked_at=timezone.now())
            # The runner rows stay: they carry the pod binding the next
            # sign-in reuses. Marking them OFFLINE is what stops dispatch.
            Runner.objects.filter(
                dev_machine_id__in=machine_ids,
                provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
            ).exclude(status=RunnerStatus.REVOKED).update(status=RunnerStatus.OFFLINE)

        logger.info("managed_runner.removed user=%s machines=%s", request.user.id, len(machine_ids))
        return Response(status=status.HTTP_204_NO_CONTENT)
