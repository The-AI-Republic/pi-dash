# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""X-Api-Key authenticated runner endpoints used by the local CLI.

The web UI's session-authenticated delete already lives in
``pi_dash.runner.views.runners.RunnerDetailEndpoint``. The CLI cannot
use that surface (no session cookie), so this thin wrapper exposes the
same delete via the ``/api/v1/`` MachineToken / X-Api-Key spine.

Both surfaces delegate to the same shared service
(``pi_dash.runner.services.runner_delete``) so the cascade-vs-cloud-only
semantics stay in lockstep.
"""

from django.core.exceptions import PermissionDenied
from rest_framework import status
from rest_framework.response import Response

from pi_dash.api.views.base import BaseAPIView
from pi_dash.db.models.project import Project
from pi_dash.runner.models import Runner
from pi_dash.runner.serializers import RunnerSerializer
from pi_dash.runner.services.permissions import can_manage_runner, can_view_runner
from pi_dash.runner.services.runner_delete import (
    delete_runner as delete_runner_svc,
    parse_purge_local,
)
from pi_dash.runner.services.runner_directory import (
    parse_include_bundled,
    project_runners_queryset,
)


class RunnerDeleteEndpoint(BaseAPIView):
    """``DELETE /api/v1/runners/<runner_id>/`` — CLI-friendly cascade delete.

    Authenticates via ``X-Api-Key`` (``APIKeyAuthentication`` from
    :mod:`pi_dash.api.middleware.api_authentication`). Authorizes via
    the same ``can_manage_runner`` predicate as the web UI: the
    caller must be able to view and manage the runner. Private runners are
    owner-only.

    Accepts the same ``?purge_local=true|false`` query flag as the
    web endpoint; default is ``true`` so a CLI that omits the flag
    still gets the cascade behaviour the operator typed
    ``pidash runner remove`` to invoke.
    """

    def delete(self, request, runner_id):
        runner = Runner.objects.filter(pk=runner_id).first()
        if runner is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not can_view_runner(request.user, runner):
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not can_manage_runner(request.user, runner):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        try:
            purge_local = parse_purge_local(request.query_params)
        except ValueError as exc:
            return Response({"error": str(exc)}, status=status.HTTP_400_BAD_REQUEST)
        delete_runner_svc(runner, purge_local=purge_local)
        return Response(status=status.HTTP_204_NO_CONTENT)


class ProjectRunnersEndpoint(BaseAPIView):
    """``GET /api/v1/workspaces/<slug>/projects/<project_id>/runners/`` —
    read-only cloud runner list for the CLI (``pidash project runners``).

    The session-authenticated twin backing the web AI Workers panel is
    ``pi_dash.runner.views.runners.RunnerListEndpoint``; both delegate the
    visibility and filter rules to
    :func:`pi_dash.runner.services.runner_directory.project_runners_queryset`
    so the two surfaces (and the MCP tool) cannot drift apart. Accepts an
    optional ``?pod=<uuid>`` filter and the shared ``include_bundled`` flag.

    ``project_id`` may arrive as a UUID or a project identifier —
    ``_rewrite_project_kwarg`` (see :mod:`pi_dash.api.views.base`) resolves
    identifiers before this method runs and 404s unknown ones. A raw UUID
    passes through that rewrite unverified, so existence in the named
    workspace is re-checked here.

    The response is the ``RunnerSerializer`` payload the web panel gets:
    identity, status, pod, host/dev-machine labels, agent metadata, version,
    heartbeat, visibility, provisioning. No secrets or tokens are part of
    that serializer and none may ever be added to this response.
    """

    def get(self, request, slug, project_id):
        project = Project.objects.filter(
            pk=project_id, workspace__slug=slug, deleted_at__isnull=True
        ).first()
        if project is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        try:
            qs = project_runners_queryset(
                request.user,
                project.workspace_id,
                project_id=project.pk,
                pod_id=request.query_params.get("pod"),
                include_bundled=parse_include_bundled(request.query_params),
            )
        except PermissionDenied:
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        return Response(RunnerSerializer(qs, many=True).data)
