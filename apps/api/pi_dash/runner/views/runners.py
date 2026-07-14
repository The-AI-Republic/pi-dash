# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import transaction
from django.db.models import Count, Max, Q
from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.models import (
    AgentRun,
    DevMachine,
    MachineToken,
    Pod,
    Runner,
    RunnerStatus,
    Visibility,
)
from pi_dash.runner.serializers import DevMachineSerializer, RunnerSerializer
from pi_dash.runner.services.matcher import NON_TERMINAL_STATUSES
from pi_dash.runner.services.permissions import (
    can_manage_runner,
    can_view_dev_machine,
    can_view_runner,
    is_workspace_member,
    runner_visible_to_user_q,
)
from pi_dash.runner.services.pubsub import (
    close_runner_session,
    send_runner_revoke,
)
from pi_dash.runner.services.runner_delete import (
    delete_runner as delete_runner_svc,
    parse_purge_local,
)


class DevMachineListEndpoint(APIView):
    """List the caller's dev machines that are attached to a workspace.

    ``DevMachine`` is user-scoped, but the product page is workspace-scoped.
    A machine appears here when it has at least one visible runner or known
    machine token in the requested workspace.
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
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        runner_machine_ids = (
            Runner.objects.filter(
                workspace_id=workspace_id,
                owner=request.user,
                visibility=Visibility.PRIVATE,
                dev_machine__isnull=False,
            )
            .values_list("dev_machine_id", flat=True)
            .distinct()
        )
        token_machine_ids = (
            MachineToken.objects.filter(
                workspace_id=workspace_id,
                user=request.user,
                dev_machine__isnull=False,
            )
            .values_list("dev_machine_id", flat=True)
            .distinct()
        )
        workspace_runner_filter = Q(
            runners__workspace_id=workspace_id,
            runners__owner=request.user,
            runners__visibility=Visibility.PRIVATE,
        )
        online_runner_filter = workspace_runner_filter & Q(
            runners__revoked_at__isnull=True,
            runners__status__in=[RunnerStatus.ONLINE, RunnerStatus.BUSY],
        )
        qs = (
            DevMachine.objects.filter(
                Q(id__in=runner_machine_ids) | Q(id__in=token_machine_ids),
                owner=request.user,
                visibility=Visibility.PRIVATE,
            )
            .annotate(
                runner_count=Count("runners", filter=workspace_runner_filter, distinct=True),
                online_runner_count=Count("runners", filter=online_runner_filter, distinct=True),
                last_heartbeat_at=Max("runners__last_heartbeat_at", filter=workspace_runner_filter),
            )
            .order_by("-last_seen_at", "-created_at")
        )
        return Response(DevMachineSerializer(qs, many=True).data)


def _request_workspace_id(request):
    return (request.data.get("workspace") or request.query_params.get("workspace") or "").strip()


def _machine_is_in_workspace_scope(user, machine: DevMachine, workspace_id) -> bool:
    """True when the workspace-scoped dev-machine page may act on machine."""
    if not can_view_dev_machine(user, machine):
        return False
    runner_exists = Runner.objects.filter(
        workspace_id=workspace_id,
        owner=user,
        visibility=Visibility.PRIVATE,
        dev_machine=machine,
    ).exists()
    token_exists = MachineToken.objects.filter(
        workspace_id=workspace_id,
        user=user,
        dev_machine=machine,
    ).exists()
    return runner_exists or token_exists


def _serialize_dev_machine(machine: DevMachine, user, workspace_id):
    workspace_runner_filter = Q(
        runners__workspace_id=workspace_id,
        runners__owner=user,
        runners__visibility=Visibility.PRIVATE,
    )
    online_runner_filter = workspace_runner_filter & Q(
        runners__revoked_at__isnull=True,
        runners__status__in=[RunnerStatus.ONLINE, RunnerStatus.BUSY],
    )
    row = (
        DevMachine.objects.filter(pk=machine.pk)
        .annotate(
            runner_count=Count("runners", filter=workspace_runner_filter, distinct=True),
            online_runner_count=Count("runners", filter=online_runner_filter, distinct=True),
            last_heartbeat_at=Max("runners__last_heartbeat_at", filter=workspace_runner_filter),
        )
        .first()
    )
    return DevMachineSerializer(row or machine).data


class DevMachineRevokeEndpoint(APIView):
    """Revoke a dev machine and invalidate all active tokens for it."""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, machine_id):
        workspace_id = _request_workspace_id(request)
        if not workspace_id:
            return Response(
                {"error": "workspace is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not is_workspace_member(request.user, workspace_id):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        with transaction.atomic():
            machine = DevMachine.objects.select_for_update().filter(pk=machine_id).first()
            if machine is None or not _machine_is_in_workspace_scope(request.user, machine, workspace_id):
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)

            now = timezone.now()
            if machine.revoked_at is None:
                machine.revoked_at = now
                machine.save(update_fields=["revoked_at", "updated_at"])
            MachineToken.objects.filter(dev_machine=machine, revoked_at__isnull=True).update(revoked_at=now)
            runners = list(Runner.objects.select_for_update().filter(dev_machine=machine, revoked_at__isnull=True))

            # Enqueue before Runner.revoke() closes sessions; this mirrors runner-delete.
            for runner in runners:
                send_runner_revoke(runner.pk, reason="dev machine revoked")
            for runner in runners:
                runner.revoke(reason="dev_machine_revoked")
                close_runner_session(runner.pk)

        return Response(_serialize_dev_machine(machine, request.user, workspace_id), status=status.HTTP_200_OK)


class DevMachineRotateEndpoint(APIView):
    """Invalidate active dev-machine tokens without revoking the machine row."""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, machine_id):
        workspace_id = _request_workspace_id(request)
        if not workspace_id:
            return Response(
                {"error": "workspace is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not is_workspace_member(request.user, workspace_id):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        with transaction.atomic():
            machine = DevMachine.objects.select_for_update().filter(pk=machine_id).first()
            if machine is None or not _machine_is_in_workspace_scope(request.user, machine, workspace_id):
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
            if machine.revoked_at is not None:
                return Response(
                    {"error": "dev_machine_revoked"},
                    status=status.HTTP_409_CONFLICT,
                )

            now = timezone.now()
            MachineToken.objects.filter(dev_machine=machine, revoked_at__isnull=True).update(revoked_at=now)
            runner_ids = list(
                Runner.objects.filter(dev_machine=machine, revoked_at__isnull=True).values_list("pk", flat=True)
            )
            for runner_id in runner_ids:
                send_runner_revoke(runner_id, reason="machine token rotated")
                close_runner_session(runner_id)

        return Response(_serialize_dev_machine(machine, request.user, workspace_id), status=status.HTTP_200_OK)


class RunnerListEndpoint(APIView):
    """List runners in a workspace.

    Private runners are visible only to their owner, even within a shared
    workspace. Optional ``?pod=<uuid>`` filter narrows to a single pod.
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
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        qs = (
            Runner.objects.filter(workspace_id=workspace_id)
            .filter(runner_visible_to_user_q(request.user))
            # ``pod__project`` and ``dev_machine`` are read by the runner
            # serializer's nested mini serializers; join them to avoid N+1.
            .select_related("pod__project", "dev_machine")
            .order_by("-updated_at")
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
        runner = (
            Runner.objects.select_related("pod__project", "dev_machine").filter(pk=runner_id).first()
        )
        if runner is None:
            return None
        if not is_workspace_member(request.user, runner.workspace_id):
            return False
        if not can_view_runner(request.user, runner):
            return None
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
                new_pod = Pod.objects.select_for_update().filter(pk=new_pod_id).first()
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
                # Refuse to move a runner that has a non-terminal run *bound*
                # to it — either one it is actively serving (``runner=``) or
                # one reserved for it (``pinned_runner=``, e.g. a queued or
                # paused follow-up). A run's pod FK is immutable, so a move
                # would strand that run in the old pod: the new pod's queue
                # can't see it (``next_for_runner`` only scans the runner's
                # current pod) and the old pod's drain skips pinned runs, so it
                # never dispatches. Unpinned queued runs are NOT bound to this
                # runner (any runner in the pod can take them), so they don't
                # block. Only a real move is guarded — re-sending the current
                # pod stays a no-op. Mirrors the issue-side reassignment guard,
                # which also keys on NON_TERMINAL_STATUSES.
                if new_pod.id != runner.pod_id and AgentRun.objects.filter(
                    Q(runner=runner) | Q(pinned_runner=runner),
                    status__in=NON_TERMINAL_STATUSES,
                ).exists():
                    return Response(
                        {
                            "error": "runner has an in-flight or queued run; wait for it to finish or cancel it first",
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


class RunnerRevokeEndpoint(APIView):
    """``POST /api/runners/<runner_id>/revoke/`` — hard-revoke without delete.

    Cascades to sessions, in-flight runs, and pinned follow-ups via
    ``Runner.revoke()``. Idempotent: a second call on an already-revoked
    row returns 200 with the current state without re-emitting the
    revoke control frame or re-closing the (already closed) session.
    Use this when an operator wants to stop a runner permanently but
    keep its history visible in the list. To attach replacement local
    execution capacity, add a new runner from the target authenticated
    machine with ``pidash runner add``.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, runner_id):
        # Hold the row lock across the read-then-revoke window so two
        # concurrent operator clicks don't both see ``revoked_at IS NULL``
        # and both fire the cascade + control frame.
        with transaction.atomic():
            runner = Runner.objects.select_for_update().filter(pk=runner_id).first()
            if runner is None:
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
            if not can_view_runner(request.user, runner):
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
            if not can_manage_runner(request.user, runner):
                return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
            already_revoked = runner.revoked_at is not None
            runner_pk = runner.pk
            if not already_revoked:
                runner.revoke(reason="manual_revoke")

        if not already_revoked:
            send_runner_revoke(runner_pk, reason="revoked by user")
            close_runner_session(runner_pk)
            runner.refresh_from_db()
        return Response(RunnerSerializer(runner).data)
