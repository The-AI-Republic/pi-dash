# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import transaction
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.models import AgentRun, Pod, Runner
from pi_dash.runner.serializers import RunnerSerializer
from pi_dash.runner.services.matcher import BUSY_STATUSES
from pi_dash.runner.services.permissions import (
    can_manage_runner,
    is_workspace_member,
)
from pi_dash.runner.services.pubsub import (
    close_runner_session,
    send_runner_revoke,
)
from pi_dash.runner.services.runner_delete import (
    delete_runner as delete_runner_svc,
    parse_purge_local,
)


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
        if not can_manage_runner(request.user, runner):
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
                # Refuse to move a runner that is actively serving a run.
                # The run's pod FK is immutable, so a move would leave the
                # active run pointing at the old pod while its runner now
                # belongs to a new one — the old pod's queue silently loses
                # the runner mid-flight, and pinned follow-ups would resolve
                # to a runner sitting in a different pod. Only block a real
                # move (re-sending the current pod stays a no-op). BUSY_STATUSES
                # is the "runner is currently serving a run" set — a paused run
                # (PAUSED_AWAITING_INPUT) frees the runner, so it doesn't block.
                if new_pod.id != runner.pod_id and AgentRun.objects.filter(
                    runner=runner, status__in=BUSY_STATUSES
                ).exists():
                    return Response(
                        {
                            "error": "runner is serving an active run; wait for it to finish or cancel it first",
                            "code": "runner_busy",
                        },
                        status=status.HTTP_409_CONFLICT,
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

        Accepts a ``?purge_local=true|false`` query flag (default:
        ``true``). When true the daemon receives a ``remove_runner``
        frame and cascades the teardown to local state (data dir +
        ``[[runner]]`` block in ``config.toml``); when false a plain
        ``revoke`` frame is emitted and the local install is left
        untouched.
        """
        runner = Runner.objects.filter(pk=runner_id).first()
        if runner is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not can_manage_runner(request.user, runner):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        try:
            purge_local = parse_purge_local(request.query_params)
        except ValueError as exc:
            return Response(
                {"error": str(exc)}, status=status.HTTP_400_BAD_REQUEST
            )
        delete_runner_svc(runner, purge_local=purge_local)
        return Response(status=status.HTTP_204_NO_CONTENT)


class RunnerRevokeEndpoint(APIView):
    """``POST /api/runners/<runner_id>/revoke/`` — hard-revoke without delete.

    Cascades to sessions, in-flight runs, and pinned follow-ups via
    ``Runner.revoke()``. Idempotent: a second call on an already-revoked
    row returns 200 with the current state without re-emitting the
    revoke control frame or re-closing the (already closed) session.
    Use this when an operator wants to stop a runner permanently but
    keep its history visible in the list — paired with the ``revive``
    endpoint that mints a fresh enrollment token on the same row.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, runner_id):
        # Hold the row lock across the read-then-revoke window so two
        # concurrent operator clicks don't both see ``revoked_at IS NULL``
        # and both fire the cascade + control frame.
        with transaction.atomic():
            runner = (
                Runner.objects.select_for_update().filter(pk=runner_id).first()
            )
            if runner is None:
                return Response(
                    {"error": "not found"}, status=status.HTTP_404_NOT_FOUND
                )
            if not can_manage_runner(request.user, runner):
                return Response(
                    {"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN
                )
            already_revoked = runner.revoked_at is not None
            runner_pk = runner.pk
            if not already_revoked:
                runner.revoke(reason="manual_revoke")

        if not already_revoked:
            send_runner_revoke(runner_pk, reason="revoked by user")
            close_runner_session(runner_pk)
            runner.refresh_from_db()
        return Response(RunnerSerializer(runner).data)
