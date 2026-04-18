# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from apple_pi_dash.runner.models import (
    AgentRunStatus,
    ApprovalRequest,
    ApprovalStatus,
)
from apple_pi_dash.runner.serializers import (
    ApprovalDecisionSerializer,
    ApprovalRequestSerializer,
)
from apple_pi_dash.runner.services.pubsub import send_to_runner


class ApprovalListEndpoint(APIView):
    permission_classes = [IsAuthenticated]

    def get(self, request):
        qs = (
            ApprovalRequest.objects.filter(agent_run__owner=request.user)
            .filter(status=ApprovalStatus.PENDING)
            .order_by("-requested_at")[:200]
        )
        return Response(ApprovalRequestSerializer(qs, many=True).data)


class ApprovalDecideEndpoint(APIView):
    permission_classes = [IsAuthenticated]

    def post(self, request, approval_id):
        serializer = ApprovalDecisionSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        try:
            approval = ApprovalRequest.objects.select_related(
                "agent_run", "agent_run__runner"
            ).get(id=approval_id, agent_run__owner=request.user)
        except ApprovalRequest.DoesNotExist:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if approval.status != ApprovalStatus.PENDING:
            return Response(
                {"error": "already decided"},
                status=status.HTTP_409_CONFLICT,
            )
        decision = serializer.validated_data["decision"]
        approval.status = (
            ApprovalStatus.ACCEPTED
            if decision != "decline"
            else ApprovalStatus.DECLINED
        )
        approval.decision_source = "web"
        approval.decided_by = request.user
        approval.decided_at = timezone.now()
        approval.save(
            update_fields=["status", "decision_source", "decided_by", "decided_at"]
        )
        # Run stays in a "working" state after approval decision.
        if approval.agent_run.status == AgentRunStatus.AWAITING_APPROVAL:
            approval.agent_run.status = AgentRunStatus.RUNNING
            approval.agent_run.save(update_fields=["status"])
        runner = approval.agent_run.runner
        if runner is not None:
            send_to_runner(
                runner.id,
                {
                    "v": 1,
                    "type": "decide",
                    "run_id": str(approval.agent_run_id),
                    "approval_id": str(approval.id),
                    "decision": decision,
                    "decided_by": str(request.user.id),
                },
            )
        return Response(ApprovalRequestSerializer(approval).data)
