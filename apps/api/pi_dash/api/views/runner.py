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

from rest_framework import status
from rest_framework.response import Response

from pi_dash.api.views.base import BaseAPIView
from pi_dash.core.permissions import is_workspace_member
from pi_dash.db.models.workspace import Workspace
from pi_dash.runner.models import Runner
from pi_dash.runner.serializers import RunnerSerializer
from pi_dash.runner.services.permissions import (
    can_manage_runner,
    can_view_runner,
    runner_visible_to_user_q,
)
from pi_dash.runner.services.runner_delete import (
    delete_runner as delete_runner_svc,
    parse_purge_local,
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


class RunnerListAPIEndpoint(BaseAPIView):
    """``GET /api/v1/workspaces/<slug>/runners/`` — CLI-friendly runner list.

    Token counterpart of the web ``RunnerListEndpoint`` (session auth,
    ``/api/runners/``): the CLI cannot use that surface (no session
    cookie), so this thin wrapper exposes the same read via the
    ``X-Api-Key`` spine. The visibility rule, filters and serializer are
    shared so the two surfaces cannot drift.

    Optional ``?project=<uuid>`` narrows to every pod owned by that
    project; optional ``?assignable_for=<issue-id | "new">`` adds the
    per-row eligibility read (``assignable``, ``not_assignable_code``,
    ``is_busy``, ``queue_depth``, dedicated-runner design §7.4).
    """

    def get(self, request, slug):
        workspace = Workspace.objects.filter(slug=slug).first()
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)
        if not is_workspace_member(request.user, workspace.id):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        qs = (
            Runner.objects.filter(workspace_id=workspace.id)
            .filter(runner_visible_to_user_q(request.user))
            .select_related("pod__project", "dev_machine")
            .order_by("-updated_at")
        )
        project_id = request.query_params.get("project")
        if project_id:
            qs = qs.filter(pod__project_id=project_id)
        context = {"request": request}
        assignable_for = request.query_params.get("assignable_for")
        if assignable_for is not None:
            from pi_dash.db.models.issue import Issue

            try:
                issue = None if assignable_for == "new" else Issue.objects.filter(pk=assignable_for).first()
            except (ValueError, TypeError):
                issue = None
                assignable_for = "new"
            context["assignable_issue"] = issue
            context["user"] = request.user
        return Response(RunnerSerializer(qs, many=True, context=context).data)
