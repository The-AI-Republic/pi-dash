# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Pod CRUD endpoints (web app, session auth).

See ``.ai_design/issue_runner/design.md`` §8.1.

Permissions:
- List / detail: any workspace member.
- Create / rename / toggle default: workspace admin OR the pod's creator.
- Soft-delete: same as create, plus the §7.2 preconditions (no runners, no
  non-terminal runs, not the workspace's last active pod).
"""

from __future__ import annotations

from django.db import transaction
from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.models import Pod, RunnerStatus
from pi_dash.runner.serializers import PodSerializer
from pi_dash.runner.services.matcher import NON_TERMINAL_STATUSES
from pi_dash.runner.services.permissions import (
    is_workspace_admin,
    is_workspace_member,
)


def _can_manage_pod(user, pod: Pod) -> bool:
    """True if ``user`` may rename / toggle / delete this pod."""
    return is_workspace_admin(user, pod.workspace_id) or pod.created_by_id == user.id


class PodListEndpoint(APIView):
    """List pods in a workspace and create new pods.

    GET ``?workspace=<uuid>`` — list active pods, any member.
    POST — create a new pod, admin only.
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
        qs = Pod.objects.filter(workspace_id=workspace_id).order_by(
            "-is_default", "created_at"
        )
        return Response(PodSerializer(qs, many=True).data)

    def post(self, request):
        workspace_id = request.data.get("workspace")
        name = (request.data.get("name") or "").strip()
        description = request.data.get("description") or ""
        if not workspace_id or not name:
            return Response(
                {"error": "workspace and name are required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not is_workspace_admin(request.user, workspace_id):
            return Response(
                {"error": "workspace admin required"},
                status=status.HTTP_403_FORBIDDEN,
            )
        pod = Pod.objects.create(
            workspace_id=workspace_id,
            name=name,
            description=description,
            created_by=request.user,
            # Manual pods are not default unless the admin toggles after.
            is_default=False,
        )
        return Response(
            PodSerializer(pod).data, status=status.HTTP_201_CREATED
        )


class PodDetailEndpoint(APIView):
    """Read, rename / toggle default, or soft-delete a pod."""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def _get_pod(self, request, pod_id):
        pod = Pod.objects.filter(pk=pod_id).first()
        if pod is None:
            return None
        if not is_workspace_member(request.user, pod.workspace_id):
            return False  # signal forbidden
        return pod

    def get(self, request, pod_id):
        pod = self._get_pod(request, pod_id)
        if pod is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if pod is False:
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        return Response(PodSerializer(pod).data)

    def patch(self, request, pod_id):
        pod = self._get_pod(request, pod_id)
        if pod is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if pod is False:
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        if not _can_manage_pod(request.user, pod):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        updates: list[str] = []
        if "name" in request.data:
            new_name = (request.data.get("name") or "").strip()
            if not new_name:
                return Response(
                    {"error": "name cannot be empty"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
            pod.name = new_name
            updates.append("name")
        if "description" in request.data:
            pod.description = request.data.get("description") or ""
            updates.append("description")
        if "is_default" in request.data:
            wants_default = bool(request.data.get("is_default"))
            if wants_default and not pod.is_default:
                # Promote: clear any existing default in this workspace first.
                with transaction.atomic():
                    Pod.objects.filter(
                        workspace_id=pod.workspace_id, is_default=True
                    ).exclude(pk=pod.pk).update(is_default=False)
                    pod.is_default = True
                    updates.append("is_default")
            elif not wants_default and pod.is_default:
                pod.is_default = False
                updates.append("is_default")

        if updates:
            pod.save(update_fields=list(set(updates + ["updated_at"])))
        return Response(PodSerializer(pod).data)

    def delete(self, request, pod_id):
        pod = self._get_pod(request, pod_id)
        if pod is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if pod is False:
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        if not _can_manage_pod(request.user, pod):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        # Run all three §7.2 guards *inside* the transaction with the pod row
        # locked, so a concurrent runner-move / run-create / sibling-delete
        # cannot slip past the checks (TOCTOU).
        with transaction.atomic():
            locked = (
                Pod.objects.select_for_update().filter(pk=pod.pk).first()
            )
            if locked is None:
                return Response(
                    {"error": "not found"}, status=status.HTTP_404_NOT_FOUND
                )
            # Revoked runners keep their pod FK but cannot accept work, so they
            # do not block deletion (matches PodSerializer.get_runner_count).
            if locked.runners.exclude(status=RunnerStatus.REVOKED).exists():
                return Response(
                    {
                        "error": "pod has runners; move or revoke them first",
                        "code": "pod_has_runners",
                    },
                    status=status.HTTP_409_CONFLICT,
                )
            if locked.agent_runs.filter(status__in=NON_TERMINAL_STATUSES).exists():
                return Response(
                    {
                        "error": "pod has non-terminal runs; cancel or wait",
                        "code": "pod_has_active_runs",
                    },
                    status=status.HTTP_409_CONFLICT,
                )
            # Last-pod guard (invariant #13).
            sibling_count = (
                Pod.objects.filter(workspace_id=locked.workspace_id)
                .exclude(pk=locked.pk)
                .count()
            )
            if sibling_count == 0:
                return Response(
                    {
                        "error": "cannot delete the last pod in a workspace; create a replacement first",
                        "code": "last_pod_in_workspace",
                    },
                    status=status.HTTP_409_CONFLICT,
                )

            locked.deleted_at = timezone.now()
            locked.is_default = False
            locked.save(update_fields=["deleted_at", "is_default", "updated_at"])
            # Sweep pointing issues.
            from pi_dash.db.models.issue import Issue

            Issue.objects.filter(assigned_pod=locked).update(assigned_pod=None)
        return Response(status=status.HTTP_204_NO_CONTENT)
