# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Project Scheduler HTTP surface.

Workspace-level: scheduler-definition CRUD (workspace admin).
Project-level:   scheduler-binding CRUD (project admin).

See ``.ai_design/project_scheduler/design.md`` §7.
"""

from __future__ import annotations

from django.conf import settings
from django.db.models import Count, Q
from django.shortcuts import get_object_or_404
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.permissions import ROLE, allow_permission
from pi_dash.app.serializers.scheduler import (
    SchedulerBindingSerializer,
    SchedulerSerializer,
)
from pi_dash.app.views.base import BaseAPIView
from pi_dash.db.models import Project, Scheduler, SchedulerBinding, Workspace


def _feature_enabled() -> bool:
    return getattr(settings, "SCHEDULER_ENABLED", True)


def _disabled_response() -> Response:
    return Response(
        {"error": "Project scheduler is disabled on this instance"},
        status=status.HTTP_404_NOT_FOUND,
    )


# --------------------------------------------------------------------- Definitions


class WorkspaceSchedulerListEndpoint(BaseAPIView):
    """GET /workspaces/<slug>/schedulers/   — list (any workspace member)
    POST /workspaces/<slug>/schedulers/    — create (workspace admin)
    """

    @allow_permission(
        allowed_roles=[ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST],
        level="WORKSPACE",
    )
    def get(self, request, slug):
        if not _feature_enabled():
            return _disabled_response()
        workspace = get_object_or_404(Workspace, slug=slug)
        schedulers = (
            Scheduler.objects.filter(workspace=workspace)
            .annotate(
                _active_binding_count=Count(
                    "bindings",
                    filter=Q(bindings__deleted_at__isnull=True),
                )
            )
            .order_by("name")
        )
        return Response(
            SchedulerSerializer(schedulers, many=True).data,
            status=status.HTTP_200_OK,
        )

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def post(self, request, slug):
        if not _feature_enabled():
            return _disabled_response()
        workspace = get_object_or_404(Workspace, slug=slug)
        serializer = SchedulerSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        scheduler = serializer.save(workspace=workspace)
        return Response(
            SchedulerSerializer(scheduler).data,
            status=status.HTTP_201_CREATED,
        )


class WorkspaceSchedulerDetailEndpoint(BaseAPIView):
    """GET    /workspaces/<slug>/schedulers/<id>/   — read (workspace admin)
    PATCH  /workspaces/<slug>/schedulers/<id>/   — update (workspace admin)
    DELETE /workspaces/<slug>/schedulers/<id>/   — soft-delete (workspace admin)
    """

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def get(self, request, slug, scheduler_id):
        if not _feature_enabled():
            return _disabled_response()
        scheduler = get_object_or_404(
            Scheduler, pk=scheduler_id, workspace__slug=slug
        )
        return Response(
            SchedulerSerializer(scheduler).data,
            status=status.HTTP_200_OK,
        )

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def patch(self, request, slug, scheduler_id):
        if not _feature_enabled():
            return _disabled_response()
        scheduler = get_object_or_404(
            Scheduler, pk=scheduler_id, workspace__slug=slug
        )
        serializer = SchedulerSerializer(scheduler, data=request.data, partial=True)
        serializer.is_valid(raise_exception=True)
        serializer.save()
        return Response(
            SchedulerSerializer(scheduler).data,
            status=status.HTTP_200_OK,
        )

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def delete(self, request, slug, scheduler_id):
        if not _feature_enabled():
            return _disabled_response()
        scheduler = get_object_or_404(
            Scheduler, pk=scheduler_id, workspace__slug=slug
        )
        scheduler.delete()  # SoftDeleteModel: sets deleted_at
        return Response(status=status.HTTP_204_NO_CONTENT)


# --------------------------------------------------------------------- Bindings


class ProjectSchedulerBindingListEndpoint(BaseAPIView):
    """GET  /workspaces/<slug>/projects/<project_id>/scheduler-bindings/  — list
    POST /workspaces/<slug>/projects/<project_id>/scheduler-bindings/  — install
    """

    @allow_permission(
        allowed_roles=[ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST],
        level="PROJECT",
    )
    def get(self, request, slug, project_id):
        if not _feature_enabled():
            return _disabled_response()
        bindings = (
            SchedulerBinding.objects.filter(
                project_id=project_id,
                workspace__slug=slug,
            )
            .select_related("scheduler", "last_run")
            .order_by("-created_at")
        )
        return Response(
            SchedulerBindingSerializer(bindings, many=True).data,
            status=status.HTTP_200_OK,
        )

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="PROJECT")
    def post(self, request, slug, project_id):
        if not _feature_enabled():
            return _disabled_response()
        project = get_object_or_404(Project, pk=project_id, workspace__slug=slug)
        scheduler_id = request.data.get("scheduler")
        scheduler = get_object_or_404(
            Scheduler,
            pk=scheduler_id,
            workspace=project.workspace,
            is_enabled=True,
        )
        serializer = SchedulerBindingSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        binding = serializer.save(
            scheduler=scheduler,
            project=project,
            workspace=project.workspace,
            actor=request.user if request.user.is_authenticated else None,
        )
        # Populate next_run_at on first save so the scanner picks it up on
        # the next minute tick. The Beat fire path also handles NULL, but
        # writing the next-fire time here surfaces it in the API response.
        from pi_dash.bgtasks.scheduler import _next_fire_from_cron
        nxt = _next_fire_from_cron(binding.cron, now=timezone.now())
        if nxt is not None and binding.next_run_at != nxt:
            binding.next_run_at = nxt
            binding.save(update_fields=["next_run_at", "updated_at"])
        return Response(
            SchedulerBindingSerializer(binding).data,
            status=status.HTTP_201_CREATED,
        )


class ProjectSchedulerBindingDetailEndpoint(BaseAPIView):
    """GET    /workspaces/<slug>/projects/<project_id>/scheduler-bindings/<bid>/
    PATCH  ...                                                                — toggle / edit
    DELETE ...                                                                — uninstall
    """

    @allow_permission(
        allowed_roles=[ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST],
        level="PROJECT",
    )
    def get(self, request, slug, project_id, binding_id):
        if not _feature_enabled():
            return _disabled_response()
        binding = get_object_or_404(
            SchedulerBinding,
            pk=binding_id,
            project_id=project_id,
            workspace__slug=slug,
        )
        return Response(
            SchedulerBindingSerializer(binding).data,
            status=status.HTTP_200_OK,
        )

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="PROJECT")
    def patch(self, request, slug, project_id, binding_id):
        if not _feature_enabled():
            return _disabled_response()
        binding = get_object_or_404(
            SchedulerBinding,
            pk=binding_id,
            project_id=project_id,
            workspace__slug=slug,
        )
        serializer = SchedulerBindingSerializer(binding, data=request.data, partial=True)
        serializer.is_valid(raise_exception=True)
        serializer.save()
        # If cron was updated, recompute next_run_at to honour the new schedule.
        if "cron" in request.data:
            from pi_dash.bgtasks.scheduler import _next_fire_from_cron
            nxt = _next_fire_from_cron(binding.cron, now=timezone.now())
            if nxt is not None:
                binding.next_run_at = nxt
                binding.save(update_fields=["next_run_at", "updated_at"])
        return Response(
            SchedulerBindingSerializer(binding).data,
            status=status.HTTP_200_OK,
        )

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="PROJECT")
    def delete(self, request, slug, project_id, binding_id):
        if not _feature_enabled():
            return _disabled_response()
        binding = get_object_or_404(
            SchedulerBinding,
            pk=binding_id,
            project_id=project_id,
            workspace__slug=slug,
        )
        binding.delete()
        return Response(status=status.HTTP_204_NO_CONTENT)
