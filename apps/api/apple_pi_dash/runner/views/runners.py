# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from apple_pi_dash.runner.models import Runner
from apple_pi_dash.runner.serializers import RunnerSerializer


class RunnerListEndpoint(APIView):
    permission_classes = [IsAuthenticated]

    def get(self, request):
        qs = Runner.objects.filter(owner=request.user).order_by("-updated_at")
        workspace_id = request.query_params.get("workspace")
        if workspace_id:
            qs = qs.filter(workspace_id=workspace_id)
        return Response(RunnerSerializer(qs, many=True).data)


class RunnerDetailEndpoint(APIView):
    permission_classes = [IsAuthenticated]

    def get(self, request, runner_id):
        try:
            runner = Runner.objects.get(id=runner_id, owner=request.user)
        except Runner.DoesNotExist:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        return Response(RunnerSerializer(runner).data)


class RunnerRevokeEndpoint(APIView):
    permission_classes = [IsAuthenticated]

    def post(self, request, runner_id):
        try:
            runner = Runner.objects.get(id=runner_id, owner=request.user)
        except Runner.DoesNotExist:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        runner.revoke()
        # Fan out a revoke WS message — best-effort; runner may already be offline.
        from apple_pi_dash.runner.services.pubsub import send_to_runner

        send_to_runner(
            runner.id,
            {"v": 1, "type": "revoke", "reason": "revoked by user"},
        )
        return Response(RunnerSerializer(runner).data)
