# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prompt-template preview endpoint.

Lets a workspace admin render a template against an issue *without* creating an
``AgentRun``. Critical for iterating on the handbook.
"""

from __future__ import annotations

import uuid

from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.workspace import Workspace, WorkspaceMember
from pi_dash.prompting.context import build_context
from pi_dash.prompting.models import PromptTemplate
from pi_dash.prompting.renderer import PromptRenderError, render


class _FakeRun:
    """Stand-in for :class:`AgentRun` that carries the fields the renderer
    consumes, so a preview never has to mutate the DB."""

    def __init__(self, run_id: uuid.UUID):
        self.id = run_id
        self.work_item_id = None


def _is_workspace_admin(user, workspace: Workspace) -> bool:
    if user.is_superuser or user.is_staff:
        return True
    return WorkspaceMember.objects.filter(
        workspace=workspace, member=user, role=20, is_active=True
    ).exists()


class PromptTemplatePreviewEndpoint(APIView):
    """``POST /api/v1/workspaces/<slug>/prompt-templates/<uuid>/preview``.

    Body: ``{"issue_id": "<uuid>"}``.
    Returns: ``{"prompt": "<rendered>"}``.

    Does **not** create an ``AgentRun`` and does not touch the runner.
    """

    permission_classes = [IsAuthenticated]

    def post(self, request, slug: str, template_id: uuid.UUID):
        try:
            workspace = Workspace.objects.get(slug=slug)
        except Workspace.DoesNotExist:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)

        if not _is_workspace_admin(request.user, workspace):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        template = (
            PromptTemplate.objects.filter(id=template_id)
            .filter(_visible_to(workspace))
            .first()
        )
        if template is None:
            return Response({"error": "template not found"}, status=status.HTTP_404_NOT_FOUND)

        issue_id = request.data.get("issue_id")
        if not issue_id:
            return Response(
                {"error": "issue_id is required"}, status=status.HTTP_400_BAD_REQUEST
            )
        try:
            issue = (
                Issue.objects.select_related("project", "workspace", "state")
                .get(id=issue_id, workspace=workspace)
            )
        except Issue.DoesNotExist:
            return Response({"error": "issue not found"}, status=status.HTTP_404_NOT_FOUND)

        context = build_context(issue, _FakeRun(run_id=uuid.uuid4()))
        try:
            rendered = render(template.body, context)
        except PromptRenderError as exc:
            return Response(
                {"error": "render failed", "detail": str(exc)},
                status=status.HTTP_422_UNPROCESSABLE_ENTITY,
            )
        return Response({"prompt": rendered})


def _visible_to(workspace):
    """Templates visible to a workspace: the workspace's own rows plus the
    global default."""
    from django.db.models import Q

    return Q(workspace=workspace) | Q(workspace__isnull=True)
