# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""External-API (`/api/v1/`) read-only scheduler endpoints.

Mirrors the web app surface in ``pi_dash/app/views/scheduler/`` but for the
API-key surface the `pidash` CLI and the MCP connector consume, and strictly
read-only (PDASHOSS01-225). Scoping:

- Workspace list: any workspace member; nested bindings are limited to
  projects the caller is an active member of.
- Project list / detail / runs: any active member of that project
  (``ProjectEntityPermission`` SAFE_METHODS); non-members get the normal
  permission error.
"""

from django.conf import settings
from django.db.models import Prefetch
from django.shortcuts import get_object_or_404
from rest_framework import status
from rest_framework.response import Response

from pi_dash.api.serializers.scheduler import (
    SchedulerAPISerializer,
    SchedulerRunAPISerializer,
)
from pi_dash.api.views.base import BaseAPIView
from pi_dash.app.permissions import ProjectEntityPermission, WorkspaceEntityPermission
from pi_dash.db.models import IssueComment, Scheduler, SchedulerBinding
from pi_dash.runner.models import AgentRun


def _feature_enabled() -> bool:
    return getattr(settings, "SCHEDULER_ENABLED", True)


def _disabled_response() -> Response:
    return Response(
        {"error": "Project scheduler is disabled on this instance"},
        status=status.HTTP_404_NOT_FOUND,
    )


def _binding_queryset():
    return (
        SchedulerBinding.objects.filter(deleted_at__isnull=True)
        .select_related("scheduler", "last_run", "pod")
        .order_by("-created_at")
    )


def _issues_by_run(runs, user) -> dict:
    """Map run id -> the issues that run wrote to (audit trail).

    The durable run→issue link is the comments the run posted
    (``IssueComment.speaker_agent_run_id``): scheduler runs report their
    findings and file follow-ups through comments made with their run id.
    One query for the whole page.

    Scoped to projects ``user`` is an active member of: a run may have
    commented on an issue in a project the caller cannot access, and issue
    visibility is project-scoped, so those rows are omitted rather than
    leaking another project's issue identifier and name (same rule as the
    nested bindings on the workspace list).
    """
    run_ids = [run.id for run in runs]
    if not run_ids:
        return {}
    rows = (
        IssueComment.objects.filter(
            speaker_agent_run_id__in=run_ids,
            issue__project__project_projectmember__member=user,
            issue__project__project_projectmember__is_active=True,
        )
        .values_list(
            "speaker_agent_run_id",
            "issue_id",
            "issue__name",
            "issue__sequence_id",
            "issue__project__identifier",
        )
        .distinct()
    )
    out: dict = {}
    for run_id, issue_id, name, sequence_id, project_identifier in rows:
        out.setdefault(run_id, []).append(
            {
                "id": str(issue_id),
                "identifier": f"{project_identifier}-{sequence_id}",
                "name": name,
            }
        )
    return out


class WorkspaceSchedulerListAPIEndpoint(BaseAPIView):
    """GET /api/v1/workspaces/<slug>/schedulers/ — list workspace schedulers."""

    model = Scheduler
    serializer_class = SchedulerAPISerializer
    permission_classes = [WorkspaceEntityPermission]
    use_read_replica = True

    def get_queryset(self):
        # Nested bindings only for projects the caller can see. The
        # related-manager traversal bypasses the soft-delete manager, so
        # the deleted_at filter is explicit.
        visible_bindings = _binding_queryset().filter(
            project__project_projectmember__member=self.request.user,
            project__project_projectmember__is_active=True,
            project__archived_at__isnull=True,
        )
        return (
            Scheduler.objects.filter(workspace__slug=self.kwargs.get("slug"))
            .prefetch_related(
                Prefetch(
                    "bindings",
                    queryset=visible_bindings,
                    to_attr="visible_bindings",
                )
            )
            .order_by("name")
        )

    def get(self, request, slug):
        if not _feature_enabled():
            return _disabled_response()
        return self.paginate(
            request=request,
            queryset=self.get_queryset(),
            on_results=lambda schedulers: SchedulerAPISerializer(
                schedulers, many=True, fields=self.fields, expand=self.expand
            ).data,
        )


class ProjectSchedulerListAPIEndpoint(BaseAPIView):
    """GET /api/v1/workspaces/<slug>/projects/<project_id>/schedulers/

    Schedulers installed on this project, each with its binding on this
    project (cadence, enabled, next and last occurrence).
    """

    model = Scheduler
    serializer_class = SchedulerAPISerializer
    permission_classes = [ProjectEntityPermission]
    use_read_replica = True

    def get_queryset(self):
        project_bindings = _binding_queryset().filter(
            project_id=self.kwargs.get("project_id")
        )
        return (
            Scheduler.objects.filter(
                workspace__slug=self.kwargs.get("slug"),
                bindings__project_id=self.kwargs.get("project_id"),
                bindings__deleted_at__isnull=True,
            )
            .prefetch_related(
                Prefetch(
                    "bindings",
                    queryset=project_bindings,
                    to_attr="visible_bindings",
                )
            )
            .distinct()
            .order_by("name")
        )

    def get(self, request, slug, project_id):
        if not _feature_enabled():
            return _disabled_response()
        return self.paginate(
            request=request,
            queryset=self.get_queryset(),
            on_results=lambda schedulers: SchedulerAPISerializer(
                schedulers, many=True, fields=self.fields, expand=self.expand
            ).data,
        )


class ProjectSchedulerDetailAPIEndpoint(BaseAPIView):
    """GET /api/v1/workspaces/<slug>/projects/<project_id>/schedulers/<scheduler_id>/

    Full definition (prompt included) of one scheduler installed on this
    project, with its binding on this project.
    """

    model = Scheduler
    serializer_class = SchedulerAPISerializer
    permission_classes = [ProjectEntityPermission]
    use_read_replica = True

    def get(self, request, slug, project_id, scheduler_id):
        if not _feature_enabled():
            return _disabled_response()
        project_bindings = _binding_queryset().filter(project_id=project_id)
        scheduler = get_object_or_404(
            Scheduler.objects.filter(
                workspace__slug=slug,
                bindings__project_id=project_id,
                bindings__deleted_at__isnull=True,
            )
            .prefetch_related(
                Prefetch(
                    "bindings",
                    queryset=project_bindings,
                    to_attr="visible_bindings",
                )
            )
            .distinct(),
            pk=scheduler_id,
        )
        return Response(
            SchedulerAPISerializer(scheduler).data,
            status=status.HTTP_200_OK,
        )


class ProjectSchedulerRunsAPIEndpoint(BaseAPIView):
    """GET /api/v1/workspaces/<slug>/projects/<project_id>/schedulers/<scheduler_id>/runs/

    Recent agent runs fired by this scheduler's install on this project,
    newest first. ``per_page`` (default 30, max 100) bounds the page; each
    run carries the issues it wrote to so its behavior can be audited.
    """

    model = AgentRun
    serializer_class = SchedulerRunAPISerializer
    permission_classes = [ProjectEntityPermission]
    use_read_replica = True

    def get_queryset(self):
        return (
            AgentRun.objects.filter(
                workspace__slug=self.kwargs.get("slug"),
                scheduler_binding__scheduler_id=self.kwargs.get("scheduler_id"),
                scheduler_binding__project_id=self.kwargs.get("project_id"),
            )
            .select_related("scheduler_binding")
            .order_by("-created_at")
        )

    def get(self, request, slug, project_id, scheduler_id):
        if not _feature_enabled():
            return _disabled_response()
        # 404 (not an empty page) for a scheduler that isn't installed on
        # this project, matching the detail endpoint.
        get_object_or_404(
            SchedulerBinding.objects.filter(deleted_at__isnull=True),
            scheduler_id=scheduler_id,
            project_id=project_id,
            workspace__slug=slug,
        )

        def _serialize(runs):
            return SchedulerRunAPISerializer(
                runs,
                many=True,
                context={"issues_by_run": _issues_by_run(runs, request.user)},
            ).data

        return self.paginate(
            request=request,
            queryset=self.get_queryset(),
            on_results=_serialize,
            default_per_page=30,
            max_per_page=100,
        )
