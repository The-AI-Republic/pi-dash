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
from django.db import transaction
from django.db.models import Count, Q
from django.shortcuts import get_object_or_404
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.permissions import ROLE, allow_permission
from pi_dash.app.serializers.scheduler import (
    SchedulerBindingDetailSerializer,
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
            Scheduler.objects.annotate(
                _active_binding_count=Count(
                    "bindings",
                    filter=Q(bindings__deleted_at__isnull=True),
                )
            ),
            pk=scheduler_id,
            workspace__slug=slug,
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
            Scheduler.objects.annotate(
                _active_binding_count=Count(
                    "bindings",
                    filter=Q(bindings__deleted_at__isnull=True),
                )
            ),
            pk=scheduler_id,
            workspace__slug=slug,
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
        # The SoftDeleteModel cascade is async (see db/mixins.py
        # ``soft_delete_related_objects``), which leaves a window where
        # active bindings still point at a soft-deleted parent. The
        # scanner already filters those out, but they remain addressable
        # via the bindings list endpoint and would survive a
        # same-slug recreate as orphans. Soft-delete bindings inline so
        # the API view of the world is consistent the moment this
        # response returns.
        now = timezone.now()
        with transaction.atomic():
            SchedulerBinding.objects.filter(
                scheduler=scheduler,
                deleted_at__isnull=True,
            ).update(deleted_at=now)
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
            .select_related("scheduler", "last_run", "pod")
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
        # Pass `project` in context so the serializer can validate that a
        # chosen `pod` belongs to this project (project is injected at save(),
        # so it isn't in validated_data at validation time).
        serializer = SchedulerBindingSerializer(
            data=request.data, context={"project": project}
        )
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
        from pi_dash.bgtasks.scheduler import _next_fire_for_binding
        nxt = _next_fire_for_binding(binding, now=timezone.now())
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
            SchedulerBinding.objects.select_related("scheduler", "last_run", "pod"),
            pk=binding_id,
            project_id=project_id,
            workspace__slug=slug,
        )
        return Response(
            SchedulerBindingDetailSerializer(binding).data,
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
        # If any RRULE-bundle field was touched, recompute next_run_at so the
        # scanner honours the new schedule on its next tick.
        if any(
            k in request.data for k in ("dtstart", "rrule", "rdates", "exdates", "tzid")
        ):
            from pi_dash.bgtasks.scheduler import _next_fire_for_binding
            binding.refresh_from_db()
            nxt = _next_fire_for_binding(binding, now=timezone.now())
            if nxt is not None:
                binding.next_run_at = nxt
                binding.save(update_fields=["next_run_at", "updated_at"])
        # Detail shape, not the list shape: the detail page PATCHes (enabled
        # toggle, edits) and reuses the response, so a list-shaped body would
        # drop resolved_prompt / run_count from its cache.
        return Response(
            SchedulerBindingDetailSerializer(binding).data,
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


class ProjectSchedulerBindingRunsEndpoint(BaseAPIView):
    """GET /workspaces/<slug>/projects/<project_id>/scheduler-bindings/<bid>/runs/

    Run history for one binding: the AgentRuns it has fired, newest first,
    page-number paginated with the same envelope as ``/api/runners/runs/``
    so the client reuses the ``IAgentRunPage`` type.

    Readable by any project member (ADMIN / MEMBER / GUEST) — the same read
    permission as the binding list/detail endpoints, whose payloads already
    surface the scheduler prompt surface. This is a dedicated endpoint rather
    than a filter on ``AgentRunListEndpoint`` because that endpoint's
    visibility rules are issue-centric (creator / issue owner / assignee /
    workspace admin) and would hide scheduler runs (``work_item=None``) from
    project admins.
    """

    @allow_permission(
        allowed_roles=[ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST],
        level="PROJECT",
    )
    def get(self, request, slug, project_id, binding_id):
        if not _feature_enabled():
            return _disabled_response()
        import math

        from pi_dash.runner.models import AgentRun
        from pi_dash.runner.serializers import AgentRunSerializer
        from pi_dash.runner.views.runs import _parse_pagination

        binding = get_object_or_404(
            SchedulerBinding,
            pk=binding_id,
            project_id=project_id,
            workspace__slug=slug,
        )
        qs = (
            AgentRun.objects.filter(scheduler_binding=binding)
            # Private-runner gate, mirroring AgentRunListEndpoint: project
            # standing never reveals a run executing on someone else's
            # private machine — only the run creator and the runner owner
            # see those rows. Runner-less runs (queued, Cloud Agent) pass.
            .filter(
                Q(runner__isnull=True)
                | Q(runner__owner=request.user)
                | Q(created_by=request.user)
            )
            # Joined by AgentRunSerializer (pod_detail, scheduler_binding_detail,
            # tool_calls); pull them up front so a page costs O(1) queries.
            .select_related("pod__project", "scheduler_binding__scheduler")
            .prefetch_related("tool_calls")
            .order_by("-created_at")
        )
        page, per_page = _parse_pagination(request.query_params)
        total_count = qs.count()
        total_pages = max(1, math.ceil(total_count / per_page))
        offset = (page - 1) * per_page
        results = qs[offset : offset + per_page]
        return Response(
            {
                "results": AgentRunSerializer(results, many=True).data,
                "count": len(results),
                "total_count": total_count,
                "total_pages": total_pages,
                "page": page,
                "per_page": per_page,
            },
            status=status.HTTP_200_OK,
        )
