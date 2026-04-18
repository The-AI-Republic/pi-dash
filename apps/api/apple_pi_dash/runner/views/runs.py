# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from apple_pi_dash.runner.models import AgentRun, AgentRunEvent, AgentRunStatus
from apple_pi_dash.runner.serializers import (
    AgentRunEventSerializer,
    AgentRunSerializer,
)
from apple_pi_dash.runner.services import matcher
from apple_pi_dash.runner.services.pubsub import send_to_runner


class AgentRunListEndpoint(APIView):
    """Create a new run from a work item, or list the authenticated user's runs."""

    permission_classes = [IsAuthenticated]

    def get(self, request):
        qs = AgentRun.objects.filter(owner=request.user).order_by("-created_at")
        workspace_id = request.query_params.get("workspace")
        if workspace_id:
            qs = qs.filter(workspace_id=workspace_id)
        return Response(AgentRunSerializer(qs[:200], many=True).data)

    def post(self, request):
        prompt = request.data.get("prompt")
        workspace_id = request.data.get("workspace")
        if not prompt or not workspace_id:
            return Response(
                {"error": "prompt and workspace are required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        run = AgentRun.objects.create(
            owner=request.user,
            workspace_id=workspace_id,
            prompt=prompt,
            run_config=request.data.get("run_config") or {},
            required_capabilities=request.data.get("required_capabilities") or [],
            work_item_id=request.data.get("work_item"),
        )
        chosen = matcher.select_runner_for_run(run)
        if chosen is not None:
            run.runner = chosen
            run.status = AgentRunStatus.ASSIGNED
            run.assigned_at = timezone.now()
            run.save(update_fields=["runner", "status", "assigned_at"])
            send_to_runner(
                chosen.id,
                {
                    "v": 1,
                    "type": "assign",
                    "run_id": str(run.id),
                    "work_item_id": str(run.work_item_id) if run.work_item_id else None,
                    "prompt": run.prompt,
                    "repo_url": run.run_config.get("repo_url"),
                    "repo_ref": run.run_config.get("repo_ref"),
                    "expected_codex_model": run.run_config.get("model"),
                    "approval_policy_overrides": run.run_config.get(
                        "approval_policy_overrides"
                    ),
                    "deadline": None,
                },
            )
        return Response(
            AgentRunSerializer(run).data,
            status=status.HTTP_201_CREATED,
        )


class AgentRunDetailEndpoint(APIView):
    permission_classes = [IsAuthenticated]

    def get(self, request, run_id):
        try:
            run = AgentRun.objects.get(id=run_id, owner=request.user)
        except AgentRun.DoesNotExist:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        include_events = request.query_params.get("include_events") == "1"
        payload = AgentRunSerializer(run).data
        if include_events:
            events = AgentRunEvent.objects.filter(agent_run=run).order_by("seq")[:500]
            payload["events"] = AgentRunEventSerializer(events, many=True).data
        return Response(payload)


class AgentRunCancelEndpoint(APIView):
    permission_classes = [IsAuthenticated]

    def post(self, request, run_id):
        try:
            run = AgentRun.objects.get(id=run_id, owner=request.user)
        except AgentRun.DoesNotExist:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if run.is_terminal:
            return Response(
                {"error": "run already terminal"},
                status=status.HTTP_409_CONFLICT,
            )
        if run.runner_id:
            send_to_runner(
                run.runner_id,
                {
                    "v": 1,
                    "type": "cancel",
                    "run_id": str(run.id),
                    "reason": request.data.get("reason", ""),
                },
            )
        run.status = AgentRunStatus.CANCELLED
        run.ended_at = timezone.now()
        run.save(update_fields=["status", "ended_at"])
        return Response(AgentRunSerializer(run).data)
