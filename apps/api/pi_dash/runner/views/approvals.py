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
from pi_dash.runner.models import (
    AgentRunStatus,
    ApprovalRequest,
    ApprovalStatus,
)
from pi_dash.runner.serializers import (
    ApprovalDecisionSerializer,
    ApprovalRequestSerializer,
)
from pi_dash.runner.services.pubsub import send_to_runner


class ApprovalListEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request):
        # Approvals are routed to the run creator (decision #6, design §5.2).
        qs = (
            ApprovalRequest.objects.filter(agent_run__created_by=request.user)
            .filter(status=ApprovalStatus.PENDING)
            .order_by("-requested_at")[:200]
        )
        return Response(ApprovalRequestSerializer(qs, many=True).data)


class ApprovalDecideEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, approval_id):
        serializer = ApprovalDecisionSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        decision = serializer.validated_data["decision"]
        runner_id = None
        with transaction.atomic():
            # select_for_update serializes with expire_stale_approvals so a
            # user click can't race the beat task to an inconsistent state.
            try:
                approval = (
                    ApprovalRequest.objects.select_for_update()
                    .select_related("agent_run", "agent_run__runner")
                    .get(id=approval_id, agent_run__created_by=request.user)
                )
            except ApprovalRequest.DoesNotExist:
                return Response(
                    {"error": "not found"}, status=status.HTTP_404_NOT_FOUND
                )
            if approval.status != ApprovalStatus.PENDING:
                return Response(
                    {"error": "already decided"},
                    status=status.HTTP_409_CONFLICT,
                )
            approval.status = (
                ApprovalStatus.ACCEPTED
                if decision == "accept"
                else ApprovalStatus.DECLINED
            )
            approval.decision_source = "web"
            approval.decided_by = request.user
            approval.decided_at = timezone.now()
            approval.save(
                update_fields=[
                    "status",
                    "decision_source",
                    "decided_by",
                    "decided_at",
                ]
            )
            # Run stays in a "working" state after approval decision.
            if approval.agent_run.status == AgentRunStatus.AWAITING_APPROVAL:
                approval.agent_run.status = AgentRunStatus.RUNNING
                approval.agent_run.save(update_fields=["status"])
            if approval.agent_run.runner_id is not None:
                runner_id = approval.agent_run.runner_id
        if runner_id is not None:
            send_to_runner(
                runner_id,
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
