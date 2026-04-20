# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prompt-template REST surface.

- ``GET    /api/workspaces/<slug>/prompt-templates`` — list templates visible
  to this workspace (the global default plus any workspace-scoped override).
  Readable by any workspace member.
- ``POST   /api/workspaces/<slug>/prompt-templates`` — create a workspace
  override. Workspace admins only. Pre-fills the body from the global default
  when the request omits ``body``.
- ``GET    /api/workspaces/<slug>/prompt-templates/<id>`` — detail. Any
  workspace member.
- ``PATCH  /api/workspaces/<slug>/prompt-templates/<id>`` — edit in place.
  Bumps ``version``. Workspace admins only. Refuses edits on the global
  default.
- ``POST   /api/workspaces/<slug>/prompt-templates/<id>/archive`` — flip
  ``is_active=False`` so the workspace falls back to the global default.
  Workspace admins only.
- ``POST   /api/workspaces/<slug>/prompt-templates/<id>/preview`` — render
  against a real issue without creating an ``AgentRun``. Workspace admins
  only.
"""

from __future__ import annotations

import uuid

from django.db.models import F, Q
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.throttling import UserRateThrottle
from rest_framework.views import APIView

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.workspace import Workspace, WorkspaceMember
from pi_dash.prompting.context import build_context
from pi_dash.prompting.models import PromptTemplate
from pi_dash.prompting.renderer import PromptRenderError, render
from pi_dash.prompting.serializers import PromptTemplateSerializer

#: Numeric value of ``WorkspaceMember.role`` for the "Admin" role. Mirrors
#: ``db.models.workspace.ROLE_CHOICES`` — kept as a named constant here so
#: access checks don't rely on an unlabelled literal.
WORKSPACE_ADMIN_ROLE = 20


class _FakeRun:
    """Stand-in for :class:`AgentRun` that carries the fields the renderer
    consumes, so a preview never has to mutate the DB."""

    def __init__(self, run_id: uuid.UUID):
        self.id = run_id
        self.work_item_id = None


def _is_workspace_admin(user, workspace: Workspace) -> bool:
    """Write access: workspace-scoped admins (role 20) plus Django superusers.
    ``is_staff`` alone is not sufficient — a staff flag doesn't imply
    membership in this workspace."""
    if user.is_superuser:
        return True
    return WorkspaceMember.objects.filter(
        workspace=workspace,
        member=user,
        role=WORKSPACE_ADMIN_ROLE,
        is_active=True,
    ).exists()


def _is_workspace_member(user, workspace: Workspace) -> bool:
    """Read access: any active member of the workspace, at any role."""
    if user.is_superuser:
        return True
    return WorkspaceMember.objects.filter(
        workspace=workspace,
        member=user,
        is_active=True,
    ).exists()


def _visible_to(workspace):
    """Templates visible to a workspace: the workspace's own rows plus the
    global default."""
    return Q(workspace=workspace) | Q(workspace__isnull=True)


def _get_workspace_or_404(slug: str):
    try:
        return Workspace.objects.get(slug=slug)
    except Workspace.DoesNotExist:
        return None


def _get_global_default_body() -> str:
    """Current body of the global default template, or empty string if the
    seed row is missing."""
    row = (
        PromptTemplate.objects.filter(
            workspace__isnull=True,
            name=PromptTemplate.DEFAULT_NAME,
            is_active=True,
        )
        .order_by("-updated_at")
        .first()
    )
    return row.body if row is not None else ""


class PromptTemplateListCreateEndpoint(APIView):
    """``GET|POST /api/workspaces/<slug>/prompt-templates``."""

    permission_classes = [IsAuthenticated]
    throttle_classes = [UserRateThrottle]

    def get(self, request, slug: str):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _is_workspace_member(request.user, workspace):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        # nulls_last=True so the workspace override (non-null workspace_id)
        # lands before the global default — the UI doesn't depend on this
        # order today but making it explicit avoids future bugs if consumers
        # start trusting the response order.
        qs = (
            PromptTemplate.objects.filter(is_active=True)
            .filter(_visible_to(workspace))
            .order_by(F("workspace_id").asc(nulls_last=True), "name")
        )
        serializer = PromptTemplateSerializer(qs, many=True)
        return Response(serializer.data)

    def post(self, request, slug: str):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _is_workspace_admin(request.user, workspace):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        # MVP scope (Option 1): one template kind per workspace — the built-in
        # "coding-task" slot that the composer actually reads. Client-supplied
        # names are ignored on purpose; accepting them would let callers
        # create rows that orchestration never loads.
        name = PromptTemplate.DEFAULT_NAME
        existing = PromptTemplate.objects.filter(
            workspace=workspace, name=name, is_active=True
        ).first()
        if existing is not None:
            return Response(
                {
                    "error": "workspace already has an active template with this name",
                    "existing_id": str(existing.id),
                },
                status=status.HTTP_409_CONFLICT,
            )

        body = request.data.get("body")
        if not body:
            body = _get_global_default_body()
        if not body:
            return Response(
                {"error": "no body provided and no global default available to copy from"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        serializer = PromptTemplateSerializer(data={"name": name, "body": body})
        serializer.is_valid(raise_exception=True)
        template = PromptTemplate.objects.create(
            workspace=workspace,
            name=serializer.validated_data["name"],
            body=serializer.validated_data["body"],
            is_active=True,
            version=1,
            updated_by=request.user,
        )
        return Response(
            PromptTemplateSerializer(template).data, status=status.HTTP_201_CREATED
        )


class PromptTemplateDetailEndpoint(APIView):
    """``GET|PATCH /api/workspaces/<slug>/prompt-templates/<id>``."""

    permission_classes = [IsAuthenticated]
    throttle_classes = [UserRateThrottle]

    def _lookup(self, workspace, template_id):
        return (
            PromptTemplate.objects.filter(id=template_id)
            .filter(_visible_to(workspace))
            .first()
        )

    def get(self, request, slug: str, template_id: uuid.UUID):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _is_workspace_member(request.user, workspace):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        template = self._lookup(workspace, template_id)
        if template is None:
            return Response({"error": "template not found"}, status=status.HTTP_404_NOT_FOUND)
        return Response(PromptTemplateSerializer(template).data)

    def patch(self, request, slug: str, template_id: uuid.UUID):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _is_workspace_admin(request.user, workspace):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        template = self._lookup(workspace, template_id)
        if template is None:
            return Response({"error": "template not found"}, status=status.HTTP_404_NOT_FOUND)
        if template.is_global_default:
            return Response(
                {"error": "the global default template is read-only on this surface"},
                status=status.HTTP_403_FORBIDDEN,
            )

        serializer = PromptTemplateSerializer(template, data=request.data, partial=True)
        serializer.is_valid(raise_exception=True)
        # Edit-in-place: bump version counter, persist new body, stamp the
        # editor. History is not retained on the row itself.
        new_body = serializer.validated_data.get("body", template.body)
        template.body = new_body
        template.version = (template.version or 0) + 1
        template.updated_by = request.user
        template.save(update_fields=["body", "version", "updated_by", "updated_at"])
        return Response(PromptTemplateSerializer(template).data)


class PromptTemplateArchiveEndpoint(APIView):
    """``POST /api/workspaces/<slug>/prompt-templates/<id>/archive``.

    Flips ``is_active=False`` on a workspace-scoped row so lookup falls back
    to the global default. Refuses to archive the global default itself.
    """

    permission_classes = [IsAuthenticated]
    throttle_classes = [UserRateThrottle]

    def post(self, request, slug: str, template_id: uuid.UUID):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _is_workspace_admin(request.user, workspace):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        template = (
            PromptTemplate.objects.filter(
                id=template_id, workspace=workspace, is_active=True
            ).first()
        )
        if template is None:
            return Response(
                {"error": "active workspace-scoped template not found"},
                status=status.HTTP_404_NOT_FOUND,
            )

        template.is_active = False
        template.updated_by = request.user
        template.save(update_fields=["is_active", "updated_by", "updated_at"])
        return Response(PromptTemplateSerializer(template).data)


class PromptTemplatePreviewEndpoint(APIView):
    """``POST /api/workspaces/<slug>/prompt-templates/<uuid>/preview``.

    Workspace-admin-gated. Renders the template against a real issue without
    creating an ``AgentRun``. Accepts an optional ``body`` override so the
    editor can preview unsaved drafts.
    """

    permission_classes = [IsAuthenticated]
    throttle_classes = [UserRateThrottle]

    def post(self, request, slug: str, template_id: uuid.UUID):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
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
        body = request.data.get("body") or template.body
        try:
            rendered = render(body, context)
        except PromptRenderError as exc:
            return Response(
                {"error": "render failed", "detail": str(exc)},
                status=status.HTTP_422_UNPROCESSABLE_ENTITY,
            )
        return Response({"prompt": rendered})
