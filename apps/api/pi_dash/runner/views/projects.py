# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Project listing for runner registration / discovery.

Two access modes:

- **Connection auth** (``X-Connection-Id`` + bearer secret) — used by
  the daemon so the CLI / TUI can show a user the projects available
  for registration without needing a session cookie. Scoped to the
  connection's workspace.
- **Session auth** — used by the cloud web UI when offering a project
  picker on a "create runner" or pod-creation form. Scoped to the
  user's workspace membership.

This endpoint surfaces only the fields a runner-side caller needs
(``id``, ``identifier``, ``name``, ``default_pod_id``, pod count). It is
not a replacement for the project app's full CRUD; for that, see
``apps/api/pi_dash/app/views/project/``.
"""

from __future__ import annotations

import uuid as _uuid

from rest_framework import status
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.db.models.project import Project
from pi_dash.db.models.workspace import WorkspaceMember
from pi_dash.runner.models import Connection, Pod
from pi_dash.runner.services import tokens


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

    Two auth modes (see module docstring):
    - ``X-Connection-Id`` + ``Authorization: Bearer <connection_secret>`` —
      scoped to the connection's workspace. Used by the daemon's
      ``pidash runner list-projects`` verb.
    - Session auth (no special headers) — scoped to the calling user's
      workspace membership. ``?workspace=<uuid>`` filters to one workspace.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes: list = []

    def get(self, request):
        # 1. Connection auth path (daemon).
        connection_id_raw = (request.headers.get("X-Connection-Id") or "").strip()
        auth = request.headers.get("Authorization", "")
        if connection_id_raw and auth.lower().startswith("bearer "):
            secret_raw = auth.split(" ", 1)[1].strip()
            try:
                connection_id = _uuid.UUID(connection_id_raw)
            except (ValueError, AttributeError):
                return Response(
                    {"error": "invalid X-Connection-Id"},
                    status=status.HTTP_401_UNAUTHORIZED,
                )
            secret_hash = tokens.hash_token(secret_raw)
            connection = Connection.objects.filter(
                id=connection_id,
                secret_hash=secret_hash,
                revoked_at__isnull=True,
                enrolled_at__isnull=False,
            ).first()
            if connection is None:
                return Response(
                    {"error": "invalid or revoked connection"},
                    status=status.HTTP_401_UNAUTHORIZED,
                )
            return Response(_serialize_projects(connection.workspace_id))

        # 2. Session auth path.
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
        # No filter: aggregate across every workspace the user is a member of.
        ws_ids = list(
            WorkspaceMember.objects.filter(member=request.user).values_list(
                "workspace_id", flat=True
            )
        )
        out: list[dict] = []
        for wid in ws_ids:
            out.extend(_serialize_projects(wid))
        return Response(out)
