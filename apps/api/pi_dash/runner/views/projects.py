# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Project listing for runner registration / discovery.

Two access modes:

- **Runner access-token auth** — used by the daemon so the CLI / TUI
  can show a user the projects available for registration. Scoped to
  the runner's workspace.
- **Session auth** — used by the cloud web UI when offering a project
  picker on a "create runner" or pod-creation form. Scoped to the
  user's workspace membership.
"""

from __future__ import annotations

from rest_framework import status
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.db.models.project import Project
from pi_dash.db.models.workspace import WorkspaceMember
from pi_dash.runner.authentication import RunnerAccessTokenAuthentication
from pi_dash.runner.models import Pod


def _serialize_projects(workspace_id) -> list[dict]:
    """Return one row per project in ``workspace_id`` with pod counts and
    the embedded pod list.

    The TUI's add-runner form needs the pod list per project so it can
    render a cascaded picker (project → pod) without a second
    round-trip on every project change. Pods are tiny (a few per
    project), so embedding is cheaper than chatty cascade fetches.

    Each project includes a ``pods`` array sorted with the default pod
    first; ``default_pod_id`` and ``pod_count`` stay on the parent for
    callers that don't care about the full list.
    """
    pods_by_project: dict = {}
    default_pod_ids: dict = {}
    for row in (
        Pod.objects.filter(workspace_id=workspace_id)
        .values("project_id", "is_default", "id", "name")
        .order_by("-is_default", "name")
    ):
        pid = row["project_id"]
        pods_by_project.setdefault(pid, []).append(
            {
                "id": str(row["id"]),
                "name": row["name"],
                "is_default": bool(row["is_default"]),
            }
        )
        if row["is_default"] and pid not in default_pod_ids:
            default_pod_ids[pid] = row["id"]
    return [
        {
            "id": str(p.id),
            "identifier": p.identifier,
            "name": p.name,
            "default_pod_id": (
                str(default_pod_ids[p.id]) if p.id in default_pod_ids else None
            ),
            "pod_count": len(pods_by_project.get(p.id, [])),
            "pods": pods_by_project.get(p.id, []),
        }
        for p in Project.objects.filter(workspace_id=workspace_id).order_by(
            "identifier"
        )
    ]


class ProjectListEndpoint(APIView):
    """GET /api/runners/projects/

    Two auth modes:
    - Bearer access-token (runner) — scoped to ``runner.workspace_id``.
    - Session auth (web UI) — scoped to caller's workspace memberships.
    """

    authentication_classes = [
        RunnerAccessTokenAuthentication,
        BaseSessionAuthentication,
    ]
    permission_classes: list = []

    def get(self, request):
        runner = getattr(request, "auth_runner", None)
        if runner is not None:
            return Response(_serialize_projects(runner.workspace_id))

        if not getattr(request.user, "is_authenticated", False):
            return Response(
                {"error": "authentication required"},
                status=status.HTTP_401_UNAUTHORIZED,
            )
        ws_filter = request.query_params.get("workspace")
        if ws_filter:
            if not WorkspaceMember.objects.filter(
                workspace_id=ws_filter, member=request.user
            ).exists():
                return Response(
                    {"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN
                )
            return Response(_serialize_projects(ws_filter))
        ws_ids = list(
            WorkspaceMember.objects.filter(member=request.user).values_list(
                "workspace_id", flat=True
            )
        )
        out: list[dict] = []
        for wid in ws_ids:
            out.extend(_serialize_projects(wid))
        return Response(out)
