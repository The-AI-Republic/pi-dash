# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import transaction
from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.models import AgentRun, AgentRunEvent, AgentRunStatus
from pi_dash.runner.serializers import (
    AgentRunEventSerializer,
    AgentRunSerializer,
)
from pi_dash.runner.services import matcher
from pi_dash.runner.services.permissions import (
    is_workspace_admin,
    is_workspace_member,
)
from pi_dash.runner.services.pubsub import send_to_runner
from pi_dash.runner.services.validation import (
    RunCreationError,
    validate_run_creation,
)


def _can_view_run(user, run: AgentRun) -> bool:
    """View is allowed for the creator, the runner's owner, or a workspace admin.

    Workspace membership is always required first — a user removed from the
    workspace must not be able to see runs there, even if they still appear as
    ``runner.owner`` (an admin bond that does not track current membership).
    """
    if not is_workspace_member(user, run.workspace_id):
        return False
    if run.created_by_id == user.id:
        return True
    if run.runner_id is not None and run.runner.owner_id == user.id:
        return True
    return is_workspace_admin(user, run.workspace_id)


def _can_cancel_run(user, run: AgentRun) -> bool:
    """Cancellation is permitted for the same set as view."""
    return _can_view_run(user, run)


class AgentRunListEndpoint(APIView):
    """List the caller's runs, or create a new one."""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request):
        # "My runs" — the runs the caller created. Workspace-scoped views are
        # available to admins via the workspace listing once we add it; for
        # MVP, list-by-creator is enough.
        qs = AgentRun.objects.filter(created_by=request.user).order_by(
            "-created_at"
        )
        workspace_id = request.query_params.get("workspace")
        if workspace_id:
            qs = qs.filter(workspace_id=workspace_id)
        return Response(AgentRunSerializer(qs[:200], many=True).data)

    def post(self, request):
        prompt = request.data.get("prompt")
        workspace_id = request.data.get("workspace")
        if not prompt:
            return Response(
                {"error": "prompt is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        try:
            ctx = validate_run_creation(
                request.user,
                workspace_id,
                work_item_id=request.data.get("work_item"),
                pod_id=request.data.get("pod"),
            )
        except RunCreationError as exc:
            return Response(
                {"error": exc.message, "code": exc.code},
                status=exc.status,
            )

        with transaction.atomic():
            run = AgentRun.objects.create(
                workspace_id=ctx.workspace_id,
                created_by=ctx.created_by,
                pod=ctx.pod,
                prompt=prompt,
                run_config=request.data.get("run_config") or {},
                required_capabilities=request.data.get("required_capabilities") or [],
                work_item_id=ctx.work_item_id,
                # Owner stays NULL until assignment (design §5.3).
            )

        # Drain the pod's queue — assigns this run (or any predecessor) to
        # an idle runner if one exists. Non-blocking on commit.
        matcher.drain_pod(ctx.pod)
        run.refresh_from_db()
        return Response(
            AgentRunSerializer(run).data,
            status=status.HTTP_201_CREATED,
        )


class AgentRunDetailEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request, run_id):
        run = (
            AgentRun.objects.select_related("runner").filter(id=run_id).first()
        )
        if run is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _can_view_run(request.user, run):
            # 404 not 403 — do not confirm run existence across workspaces.
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        include_events = request.query_params.get("include_events") == "1"
        payload = AgentRunSerializer(run).data
        if include_events:
            events = AgentRunEvent.objects.filter(agent_run=run).order_by("seq")[:500]
            payload["events"] = AgentRunEventSerializer(events, many=True).data
        return Response(payload)


class AgentRunCancelEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, run_id):
        # Authorization check happens on a non-locked read; re-check terminal
        # state after acquiring the row lock to avoid racing with
        # Runner.revoke() (which holds select_for_update on in-flight runs).
        run = (
            AgentRun.objects.select_related("runner").filter(id=run_id).first()
        )
        if run is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _can_cancel_run(request.user, run):
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)

        runner_id = run.runner_id
        with transaction.atomic():
            locked = (
                AgentRun.objects.select_for_update()
                .filter(id=run_id)
                .first()
            )
            if locked is None:
                return Response(
                    {"error": "not found"}, status=status.HTTP_404_NOT_FOUND
                )
            if locked.is_terminal:
                return Response(
                    {"error": "run already terminal"},
                    status=status.HTTP_409_CONFLICT,
                )
            locked.status = AgentRunStatus.CANCELLED
            locked.ended_at = timezone.now()
            locked.save(update_fields=["status", "ended_at"])
            run = locked

        # Best-effort WS fan-out after commit; runner may already be offline or
        # revoked, in which case the frame is dropped silently.
        if runner_id:
            transaction.on_commit(
                lambda rid=runner_id, reason=request.data.get("reason", ""): send_to_runner(
                    rid,
                    {
                        "v": 1,
                        "type": "cancel",
                        "run_id": str(run_id),
                        "reason": reason,
                    },
                )
            )
        return Response(AgentRunSerializer(run).data)
