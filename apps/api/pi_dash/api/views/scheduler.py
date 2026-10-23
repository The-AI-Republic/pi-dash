# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""External-API (`/api/v1/`) scheduler endpoints.

Mirrors the web app surface in ``pi_dash/app/views/scheduler/`` but for the
API-key surface the `pidash` CLI and the MCP connector consume. Reads came
with PDASHOSS01-225; PDASHOSS01-278 added the two binding writes (edit, run
now) and the definition edit (prompt, name, description); PDASHOSS01-318
added creating and deleting a definition and installing and uninstalling
it on a project. Scoping:

- Workspace list: any workspace member; nested bindings are limited to
  projects the caller is an active member of.
- Project list / detail / runs: any active member of that project
  (``ProjectEntityPermission`` SAFE_METHODS); non-members get the normal
  permission error.
- Install / binding edit / run now / uninstall: project admins only — the
  same ``allow_permission`` rule as the web app's binding routes.
- Definition create / edit / delete: workspace admins only — the same rule
  as the web app's definition routes — and never a built-in definition.
"""

import uuid

from django.conf import settings
from django.db import IntegrityError, transaction
from django.db.models import Prefetch
from django.utils.dateparse import parse_datetime
from django.shortcuts import get_object_or_404
from rest_framework import status
from rest_framework.response import Response

from pi_dash.api.serializers.scheduler import (
    SchedulerAPISerializer,
    SchedulerBindingAPISerializer,
    SchedulerRunAPISerializer,
)
from pi_dash.api.views.base import BaseAPIView
from pi_dash.app.permissions import (
    ROLE,
    ProjectEntityPermission,
    WorkspaceEntityPermission,
    allow_permission,
)
from pi_dash.app.serializers.scheduler import (
    SchedulerBindingSerializer,
    SchedulerSerializer,
)
from pi_dash.db.models import (
    IssueComment,
    Project,
    Scheduler,
    SchedulerBinding,
    Workspace,
)
from pi_dash.runner.models import AgentRun
from pi_dash.scheduler.binding_writes import (
    CADENCE_FIELDS,
    install_binding,
    mark_due_now,
    run_now_conflict,
    update_binding,
)
from pi_dash.scheduler.definition_writes import (
    create_definition,
    delete_definition,
    is_builtin,
    is_builtin_slug,
    update_definition,
)

#: Binding fields an API-key caller may edit. Everything else on the binding
#: is identity (``scheduler`` / ``project``) or runtime state, and the
#: definition's own fields (``prompt`` …) are not on the binding at all.
#: Intersected with the app serializer so a field dropped there stops being
#: writable here too.
BINDING_WRITABLE_FIELDS = tuple(
    f
    for f in (
        "extra_context",
        "enabled",
        "dtstart",
        "rrule",
        "tzid",
        "rdates",
        "exdates",
        "outcome_mode",
        "pod",
    )
    if f in SchedulerBindingSerializer.Meta.fields
)

#: Definition fields an API-key caller may edit. The app serializer also
#: takes ``slug``; renaming the slug stays web-app only.
DEFINITION_WRITABLE_FIELDS = tuple(
    f
    for f in ("prompt", "name", "description", "color", "is_enabled")
    if f in SchedulerSerializer.Meta.fields
)

#: Definition fields an API-key caller may set on create: the editable ones
#: plus the slug. ``source`` and the rest are server-set.
DEFINITION_CREATE_FIELDS = tuple(
    f
    for f in ("slug", *DEFINITION_WRITABLE_FIELDS)
    if f in SchedulerSerializer.Meta.fields
)

#: Fields an API-key caller may set on install: the scheduler (id or slug)
#: plus everything editable on a binding.
BINDING_CREATE_FIELDS = ("scheduler", *BINDING_WRITABLE_FIELDS)

#: 409 body for a write to a built-in definition.
BUILTIN_CONFLICT = {
    "error": "Built-in schedulers cannot be edited or deleted; put "
    "per-project instructions in the binding's extra_context instead.",
    "code": "builtin_scheduler",
}

#: Optional optimistic-concurrency token in a write's body: the written
#: row's ``updated_at`` as the caller last read it. Same meaning as
#: ``If-Match``.
EXPECTED_UPDATED_AT = "expected_updated_at"


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


    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def post(self, request, slug):
        """POST /api/v1/workspaces/<slug>/schedulers/ — create a definition.

        Validation is the web app's (``create_definition``), so a duplicate
        slug is a ``{"slug": [...]}`` field error. A built-in slug is
        refused: the reseed would take the row over.
        """
        if not _feature_enabled():
            return _disabled_response()
        if not isinstance(request.data, dict):
            return _non_object_response()
        data = dict(request.data)
        unknown = sorted(k for k in data if k not in DEFINITION_CREATE_FIELDS)
        if unknown:
            return _unwritable_response(
                data, DEFINITION_CREATE_FIELDS, "a new scheduler"
            )
        if isinstance(data.get("slug"), str) and is_builtin_slug(data["slug"].strip()):
            return Response(
                {"slug": ["This slug is reserved for a built-in scheduler."]},
                status=status.HTTP_400_BAD_REQUEST,
            )
        workspace = get_object_or_404(Workspace, slug=slug)
        scheduler = create_definition(workspace, data)
        scheduler.visible_bindings = []
        return Response(
            SchedulerAPISerializer(scheduler).data, status=status.HTTP_201_CREATED
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


    @allow_permission(allowed_roles=[ROLE.ADMIN], level="PROJECT")
    def post(self, request, slug, project_id):
        """POST /api/v1/workspaces/<slug>/projects/<project_id>/schedulers/

        Install a scheduler (``scheduler``: its id or slug) on this project.
        Validation and the first ``next_run_at`` are the web app's
        (``install_binding``); the caller is recorded as the install's actor.
        """
        if not _feature_enabled():
            return _disabled_response()
        if not isinstance(request.data, dict):
            return _non_object_response()
        data = dict(request.data)
        unknown = sorted(k for k in data if k not in BINDING_CREATE_FIELDS)
        if unknown:
            return _unwritable_response(
                data, BINDING_CREATE_FIELDS, "a scheduler install"
            )
        ref = data.get("scheduler")
        if not isinstance(ref, str) or not ref.strip():
            return Response(
                {"scheduler": ["The scheduler's id or slug is required."]},
                status=status.HTTP_400_BAD_REQUEST,
            )
        project = get_object_or_404(Project, pk=project_id, workspace__slug=slug)
        scheduler = _scheduler_by_ref(project.workspace_id, ref.strip())
        if scheduler is None:
            return Response(
                {"error": "Scheduler not found"}, status=status.HTTP_404_NOT_FOUND
            )
        if not scheduler.is_enabled:
            return Response(
                {
                    "error": "The scheduler definition is disabled; enable it "
                    "before installing it.",
                    "code": "scheduler_disabled",
                },
                status=status.HTTP_409_CONFLICT,
            )
        already = Response(
            {
                "error": "The scheduler is already installed on this project.",
                "code": "already_installed",
            },
            status=status.HTTP_409_CONFLICT,
        )
        if SchedulerBinding.objects.filter(
            scheduler=scheduler, project=project, deleted_at__isnull=True
        ).exists():
            return already
        try:
            with transaction.atomic():
                # The serializer's identity fields come from the URL and the
                # resolved scheduler, never from the body.
                binding = install_binding(
                    project,
                    scheduler,
                    {**data, "scheduler": scheduler.pk, "project": project.pk},
                    actor=request.user,
                )
        except IntegrityError:
            # A concurrent install won the per-project unique constraint.
            return already
        return Response(
            SchedulerBindingAPISerializer(_binding_queryset().get(pk=binding.pk)).data,
            status=status.HTTP_201_CREATED,
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


def _non_object_response() -> Response:
    return Response(
        {"error": "The request body must be a JSON object"},
        status=status.HTTP_400_BAD_REQUEST,
    )


def _scheduler_by_ref(workspace_id, ref: str):
    """The workspace's live scheduler whose id or slug is ``ref``, or None."""
    schedulers = Scheduler.objects.filter(workspace_id=workspace_id)
    try:
        return schedulers.filter(pk=uuid.UUID(ref)).first()
    except ValueError:
        return schedulers.filter(slug=ref).first()


def _locked_binding(slug, project_id, scheduler_id) -> SchedulerBinding:
    """The scheduler's install on the project, row-locked for a write."""
    return get_object_or_404(
        SchedulerBinding.objects.select_for_update(of=("self",))
        .filter(deleted_at__isnull=True)
        .select_related("scheduler"),
        scheduler_id=scheduler_id,
        project_id=project_id,
        workspace__slug=slug,
    )


def _unwritable_response(data: dict, writable: tuple, noun: str):
    """A 400 ``Response`` when ``data`` is empty or names a field outside
    ``writable``; ``None`` when every key may be written."""
    unknown = sorted(k for k in data if k not in writable)
    if not unknown and data:
        return None
    return Response(
        {
            "error": (
                f"Not writable on {noun}: {', '.join(unknown)}"
                if unknown
                else "No fields to update"
            ),
            "writable_fields": list(writable),
        },
        status=status.HTTP_400_BAD_REQUEST,
    )


def _binding_payload(binding: SchedulerBinding, previous: dict) -> dict:
    """Write response: the binding as reads return it, plus ``previous``.

    ``previous`` holds the pre-write value of every field the write
    touched, so a bad edit can be reverted by sending it back. The external
    API request log keeps the response body, which makes that row the audit
    record of the change.
    """
    fresh = _binding_queryset().get(pk=binding.pk)
    return {**SchedulerBindingAPISerializer(fresh).data, "previous": previous}


def _precondition_failure(request, row, noun: str = "scheduler binding"):
    """A 4xx ``Response`` when the caller's concurrency token is stale or
    unreadable; ``None`` when there is no token or it matches. ``row`` is
    the binding or definition being written, ``noun`` its name in errors."""
    raw = request.headers.get("If-Match")
    if raw is None and isinstance(request.data, dict):
        raw = request.data.get(EXPECTED_UPDATED_AT)
    if raw is None:
        return None
    raw = str(raw).strip().strip('"')
    if raw == "*":
        return None
    try:
        expected = parse_datetime(raw)
    except ValueError:
        # Well-formed but out of range (month 13, a 25-hour offset).
        expected = None
    if expected is None or expected.tzinfo is None:
        return Response(
            {
                "error": f"If-Match / expected_updated_at must be the {noun}'s "
                "updated_at, as an ISO 8601 datetime with an offset"
            },
            status=status.HTTP_400_BAD_REQUEST,
        )
    if expected != row.updated_at:
        return Response(
            {
                "error": f"The {noun} changed since it was read",
                "updated_at": row.updated_at.isoformat(),
            },
            status=status.HTTP_412_PRECONDITION_FAILED,
        )
    return None


class ProjectSchedulerBindingAPIEndpoint(BaseAPIView):
    """PATCH /api/v1/workspaces/<slug>/projects/<project_id>/schedulers/<scheduler_id>/binding/

    Edit the scheduler's install on this project: ``extra_context``,
    ``enabled``, the cadence, ``outcome_mode`` and ``pod``. Validation and
    the ``next_run_at`` recompute are the web app's (``update_binding``).
    """

    model = SchedulerBinding
    serializer_class = SchedulerBindingAPISerializer
    permission_classes = [ProjectEntityPermission]

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="PROJECT")
    def patch(self, request, slug, project_id, scheduler_id):
        if not _feature_enabled():
            return _disabled_response()
        if not isinstance(request.data, dict):
            return _non_object_response()
        data = {k: v for k, v in request.data.items() if k != EXPECTED_UPDATED_AT}
        # Reject rather than ignore: a caller sending `prompt` or
        # `next_run_at` should learn it did nothing, not get a 200.
        invalid = _unwritable_response(
            data, BINDING_WRITABLE_FIELDS, "a scheduler binding"
        )
        if invalid is not None:
            return invalid
        with transaction.atomic():
            binding = _locked_binding(slug, project_id, scheduler_id)
            stale = _precondition_failure(request, binding)
            if stale is not None:
                return stale
            before = SchedulerBindingAPISerializer(binding).data
            previous = {k: before[k] for k in data}
            if any(k in data for k in CADENCE_FIELDS):
                # A cadence edit moves next_run_at too; record where it was.
                previous["next_run_at"] = before["next_run_at"]
            update_binding(binding, data)
        return Response(
            _binding_payload(binding, previous), status=status.HTTP_200_OK
        )


    @allow_permission(allowed_roles=[ROLE.ADMIN], level="PROJECT")
    def delete(self, request, slug, project_id, scheduler_id):
        """DELETE .../schedulers/<scheduler_id>/binding/ — uninstall.

        Soft-deletes the install, as the web app's uninstall does; the
        definition and its installs on other projects are untouched.
        """
        if not _feature_enabled():
            return _disabled_response()
        with transaction.atomic():
            binding = _locked_binding(slug, project_id, scheduler_id)
            stale = _precondition_failure(request, binding)
            if stale is not None:
                return stale
            binding.delete()
        return Response(status=status.HTTP_204_NO_CONTENT)


class ProjectSchedulerRunNowAPIEndpoint(BaseAPIView):
    """POST /api/v1/workspaces/<slug>/projects/<project_id>/schedulers/<scheduler_id>/run-now/

    Make the binding due so the scanner fires it on its next tick; the
    regular schedule resumes afterwards. 409 when that tick could not
    produce a run (disabled, a run already active, schedule exhausted).
    """

    model = SchedulerBinding
    serializer_class = SchedulerBindingAPISerializer
    permission_classes = [ProjectEntityPermission]

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="PROJECT")
    def post(self, request, slug, project_id, scheduler_id):
        if not _feature_enabled():
            return _disabled_response()
        with transaction.atomic():
            binding = _locked_binding(slug, project_id, scheduler_id)
            stale = _precondition_failure(request, binding)
            if stale is not None:
                return stale
            conflict = run_now_conflict(binding)
            if conflict is not None:
                code, message = conflict
                return Response(
                    {"error": message, "code": code},
                    status=status.HTTP_409_CONFLICT,
                )
            previous = {
                "next_run_at": SchedulerBindingAPISerializer(binding).data[
                    "next_run_at"
                ]
            }
            mark_due_now(binding)
        return Response(
            _binding_payload(binding, previous), status=status.HTTP_200_OK
        )


class WorkspaceSchedulerDetailAPIEndpoint(BaseAPIView):
    """PATCH /api/v1/workspaces/<slug>/schedulers/<scheduler_id>/

    Edit a scheduler definition's ``prompt``, ``name`` or ``description``.
    Validation is the web app's (``update_definition``). The definition is
    shared, so the response names every project whose install the edit
    reaches. Built-in definitions are reseeded and cannot be edited.
    """

    model = Scheduler
    serializer_class = SchedulerAPISerializer
    permission_classes = [WorkspaceEntityPermission]

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def patch(self, request, slug, scheduler_id):
        if not _feature_enabled():
            return _disabled_response()
        if not isinstance(request.data, dict):
            return _non_object_response()
        data = {k: v for k, v in request.data.items() if k != EXPECTED_UPDATED_AT}
        invalid = _unwritable_response(
            data, DEFINITION_WRITABLE_FIELDS, "a scheduler definition"
        )
        if invalid is not None:
            return invalid
        with transaction.atomic():
            scheduler = get_object_or_404(
                Scheduler.objects.select_for_update(),
                pk=scheduler_id,
                workspace__slug=slug,
            )
            if is_builtin(scheduler):
                return Response(BUILTIN_CONFLICT, status=status.HTTP_409_CONFLICT)
            stale = _precondition_failure(request, scheduler, "scheduler")
            if stale is not None:
                return stale
            previous = {k: getattr(scheduler, k) for k in data}
            update_definition(scheduler, data)
        # Every live install, not only the caller's projects: the caller is
        # a workspace admin and this is the blast radius of the edit.
        bindings = list(
            _binding_queryset()
            .filter(scheduler=scheduler)
            .select_related("project")
            .order_by("project__name")
        )
        scheduler.visible_bindings = bindings
        return Response(
            {
                **SchedulerAPISerializer(scheduler).data,
                "affected_projects": [
                    {
                        "id": str(b.project_id),
                        "identifier": b.project.identifier,
                        "name": b.project.name,
                        "binding_enabled": b.enabled,
                    }
                    for b in bindings
                ],
                "previous": previous,
            },
            status=status.HTTP_200_OK,
        )

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def delete(self, request, slug, scheduler_id):
        """DELETE /api/v1/workspaces/<slug>/schedulers/<scheduler_id>/

        Soft-delete a user-created definition and every install of it, as
        the web app's delete does (``delete_definition``). Built-in
        definitions would be recreated by the reseed and are refused.
        """
        if not _feature_enabled():
            return _disabled_response()
        with transaction.atomic():
            scheduler = get_object_or_404(
                Scheduler.objects.select_for_update(),
                pk=scheduler_id,
                workspace__slug=slug,
            )
            if is_builtin(scheduler):
                return Response(BUILTIN_CONFLICT, status=status.HTTP_409_CONFLICT)
            stale = _precondition_failure(request, scheduler, "scheduler")
            if stale is not None:
                return stale
            delete_definition(scheduler)
        return Response(status=status.HTTP_204_NO_CONTENT)
