# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance-admin runner endpoints.

Read-only, cross-workspace, behind :class:`InstanceAdminPermission` — the
support surface required by ``.ai_design/managed_runner/design.md`` §15.1 and
§23 item 8: per-runner ``provisioning``, ``runner_version`` and
``dev_metadata.codex_version`` answer "which build is this user on".
"""

from __future__ import annotations

from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.views.base import BaseAPIView
from pi_dash.license.api.permissions import InstanceAdminPermission
from pi_dash.runner.models import Runner, RunnerProvisioning, RunnerStatus


class InstanceRunnerListEndpoint(BaseAPIView):
    permission_classes = [InstanceAdminPermission]

    def get(self, request):
        qs = Runner.objects.select_related("workspace", "owner").order_by("-created_at")

        provisioning = request.query_params.get("provisioning")
        # A present-but-empty value is rejected like any other unknown value,
        # rather than silently returning everything.
        if provisioning is not None:
            if provisioning not in RunnerProvisioning.values:
                return Response(
                    {"error": "invalid_provisioning", "detail": sorted(RunnerProvisioning.values)},
                    status=status.HTTP_400_BAD_REQUEST,
                )
            qs = qs.filter(provisioning=provisioning)
        runner_status = request.query_params.get("status")
        if runner_status is not None:
            if runner_status not in RunnerStatus.values:
                return Response(
                    {"error": "invalid_status", "detail": sorted(RunnerStatus.values)},
                    status=status.HTTP_400_BAD_REQUEST,
                )
            qs = qs.filter(status=runner_status)
        workspace = request.query_params.get("workspace")
        if workspace:
            qs = qs.filter(workspace__slug=workspace)

        try:
            page = max(1, int(request.query_params.get("page", 1)))
        except (TypeError, ValueError):
            page = 1
        per = 50
        start = (page - 1) * per
        rows = list(qs[start : start + per])
        return Response(
            {
                "page": page,
                "total": qs.count(),
                "results": [self._row(r) for r in rows],
            }
        )

    @staticmethod
    def _row(r: Runner) -> dict:
        dev_metadata = r.dev_metadata or {}
        return {
            "id": str(r.id),
            "name": r.name,
            "workspace_slug": r.workspace.slug if r.workspace_id else None,
            "owner_email": r.owner.email if r.owner_id else None,
            "host_label": r.host_label,
            "provisioning": r.provisioning,
            "status": r.status,
            "runner_version": r.runner_version,
            "codex_version": dev_metadata.get("codex_version"),
            "last_heartbeat_at": r.last_heartbeat_at.isoformat() if r.last_heartbeat_at else None,
            "created_at": r.created_at.isoformat() if r.created_at else None,
            "revoked_at": r.revoked_at.isoformat() if r.revoked_at else None,
        }
