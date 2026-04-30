# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import transaction
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.models import Pod, Runner
from pi_dash.runner.serializers import RunnerSerializer
from pi_dash.runner.services.permissions import (
    is_workspace_admin,
    is_workspace_member,
)
from pi_dash.runner.services.pubsub import close_runner_session, send_to_runner


def _can_manage_runner(user, runner: Runner) -> bool:
    return runner.owner_id == user.id or is_workspace_admin(user, runner.workspace_id)


class RunnerListEndpoint(APIView):
    """List runners in a workspace.

    Per design §5, listing is workspace-scoped: any workspace member can see
    every runner in that workspace, regardless of who owns it. Optional
    ``?pod=<uuid>`` filter narrows to a single pod.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request):
        workspace_id = request.query_params.get("workspace")
        if not workspace_id:
            return Response(
                {"error": "workspace is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not is_workspace_member(request.user, workspace_id):
            return Response(
                {"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN
            )
        qs = Runner.objects.filter(workspace_id=workspace_id).order_by(
            "-updated_at"
        )
        pod_id = request.query_params.get("pod")
        if pod_id:
            qs = qs.filter(pod_id=pod_id)
        return Response(RunnerSerializer(qs, many=True).data)


class RunnerDetailEndpoint(APIView):
    """Read a single runner; supports PATCH for pod move and rename."""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def _get_runner(self, request, runner_id):
        runner = Runner.objects.filter(pk=runner_id).first()
        if runner is None:
            return None
        if not is_workspace_member(request.user, runner.workspace_id):
            return False
        return runner

    def get(self, request, runner_id):
        runner = self._get_runner(request, runner_id)
        if runner is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if runner is False:
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        return Response(RunnerSerializer(runner).data)

    def patch(self, request, runner_id):
        runner = self._get_runner(request, runner_id)
        if runner is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if runner is False:
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        if not _can_manage_runner(request.user, runner):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        updates: list[str] = []
        if "name" in request.data:
            new_name = (request.data.get("name") or "").strip()
            if not new_name:
                return Response(
                    {"error": "name cannot be empty"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
            runner.name = new_name
            updates.append("name")
        new_pod_id = request.data.get("pod") if "pod" in request.data else None
        # Hold the pod lock for the duration of the runner save so a concurrent
        # pod soft-delete cannot leave the runner pointing at an inactive pod.
        if "pod" in request.data:
            with transaction.atomic():
                new_pod = (
                    Pod.objects.select_for_update()
                    .filter(pk=new_pod_id)
                    .first()
                )
                if new_pod is None:
                    return Response(
                        {"error": "pod does not exist or has been deleted"},
                        status=status.HTTP_400_BAD_REQUEST,
                    )
                if new_pod.workspace_id != runner.workspace_id:
                    return Response(
                        {"error": "pod is in a different workspace"},
                        status=status.HTTP_400_BAD_REQUEST,
                    )
                runner.pod = new_pod
                updates.append("pod")
                runner.save(update_fields=list(set(updates + ["updated_at"])))
        elif updates:
            runner.save(update_fields=list(set(updates + ["updated_at"])))
        return Response(RunnerSerializer(runner).data)

    def delete(self, request, runner_id):
        """Hard-delete a runner. Cancels in-flight runs first so historic
        AgentRuns are tombstoned (status=cancelled, ended_at set), then
        drops the row. AgentRun.runner is ``SET_NULL`` so historic runs
        survive with a null FK.
        """
        runner = Runner.objects.filter(pk=runner_id).first()
        if runner is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _can_manage_runner(request.user, runner):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        runner_pk = runner.pk
        runner.revoke()
        # Don't stamp ``v`` here — the consumer's ``_send_envelope`` adds the
        # current PROTOCOL_VERSION on every outbound frame. Hard-coding a
        # stale value would override that and ship a wrong wire version.
        send_to_runner(
            runner_pk,
            {"type": "revoke", "reason": "deleted by user"},
        )
        close_runner_session(runner_pk)
        Runner.objects.filter(pk=runner_pk).delete()
        return Response(status=status.HTTP_204_NO_CONTENT)
