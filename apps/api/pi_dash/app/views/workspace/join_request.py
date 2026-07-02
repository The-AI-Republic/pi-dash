# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Django imports
from django.core.exceptions import ValidationError
from django.core.validators import validate_email
from django.utils import timezone

# Third party modules
from rest_framework import status
from rest_framework.response import Response

# Module imports
from pi_dash.app.permissions import WorkSpaceAdminPermission
from pi_dash.app.serializers import WorkspaceJoinRequestSerializer
from pi_dash.app.views.base import BaseViewSet
from pi_dash.bgtasks.event_tracking_task import track_event
from pi_dash.db.models import Profile, Workspace, WorkspaceJoinRequest, WorkspaceMember
from pi_dash.utils.analytics_events import USER_JOINED_WORKSPACE
from pi_dash.utils.cache import invalidate_cache

# Workspace role: Admin
ADMIN_ROLE = 20


class UserWorkspaceJoinRequestViewSet(BaseViewSet):
    """Requester-facing endpoint.

    A signed-in user who has no workspace can create a join request by typing a
    workspace admin's email, and list their own requests. Lives under
    ``/users/me/`` so it is reachable by a user who has no workspace slug yet.
    """

    serializer_class = WorkspaceJoinRequestSerializer
    model = WorkspaceJoinRequest

    def get_queryset(self):
        return self.filter_queryset(
            super().get_queryset().filter(requester=self.request.user).select_related("workspace", "requester")
        )

    def create(self, request):
        admin_email = (request.data.get("admin_email") or "").strip().lower()
        message = request.data.get("message")

        # Validate the email format
        try:
            validate_email(admin_email)
        except ValidationError:
            return Response(
                {"error": "A valid workspace admin email is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        # A user cannot request to join using their own email
        if request.user.email and admin_email == request.user.email.strip().lower():
            return Response(
                {"error": "You cannot request to join a workspace using your own email"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        # Resolve the workspaces the typed email administers: an active Admin
        # member, or the workspace owner.
        target_workspace_ids = set(
            WorkspaceMember.objects.filter(member__email=admin_email, role=ADMIN_ROLE, is_active=True).values_list(
                "workspace_id", flat=True
            )
        )
        target_workspace_ids.update(Workspace.objects.filter(owner__email=admin_email).values_list("id", flat=True))

        # Never target a workspace the requester is already an active member of.
        already_member_ids = set(
            WorkspaceMember.objects.filter(
                member=request.user, workspace_id__in=target_workspace_ids, is_active=True
            ).values_list("workspace_id", flat=True)
        )
        target_workspace_ids -= already_member_ids

        if target_workspace_ids:
            for workspace_id in target_workspace_ids:
                # Idempotent: do not duplicate a pending request to a workspace.
                already_pending = WorkspaceJoinRequest.objects.filter(
                    requester=request.user,
                    workspace_id=workspace_id,
                    status=WorkspaceJoinRequest.Status.PENDING,
                ).exists()
                if already_pending:
                    continue
                WorkspaceJoinRequest.objects.create(
                    requester=request.user,
                    workspace_id=workspace_id,
                    admin_email=admin_email,
                    message=message,
                    created_by=request.user,
                )
        else:
            # The email did not resolve to any workspace admin. Record an
            # unresolved request (workspace=None) so the requester still lands in
            # the onboarding "pending" state — identical to the resolved case, so
            # an outsider cannot tell whether the email was a real admin.
            already_pending = WorkspaceJoinRequest.objects.filter(
                requester=request.user,
                workspace__isnull=True,
                admin_email=admin_email,
                status=WorkspaceJoinRequest.Status.PENDING,
            ).exists()
            if not already_pending:
                WorkspaceJoinRequest.objects.create(
                    requester=request.user,
                    workspace=None,
                    admin_email=admin_email,
                    message=message,
                    created_by=request.user,
                )

        # Always return the same neutral response regardless of resolution.
        return Response({"message": "Request sent"}, status=status.HTTP_201_CREATED)


class WorkspaceJoinRequestViewSet(BaseViewSet):
    """Admin-facing endpoint.

    List the pending join requests for a workspace and approve / deny them.
    """

    serializer_class = WorkspaceJoinRequestSerializer
    model = WorkspaceJoinRequest
    permission_classes = [WorkSpaceAdminPermission]

    def get_queryset(self):
        return self.filter_queryset(
            super()
            .get_queryset()
            .filter(
                workspace__slug=self.kwargs.get("slug"),
                status=WorkspaceJoinRequest.Status.PENDING,
            )
            .select_related("workspace", "requester")
        )

    @invalidate_cache(path="/api/workspaces/", user=False)
    @invalidate_cache(path="/api/users/me/workspaces/", multiple=True)
    @invalidate_cache(
        path="/api/workspaces/:slug/members/",
        user=False,
        multiple=True,
        url_params=True,
    )
    def approve(self, request, slug, pk):
        join_request = WorkspaceJoinRequest.objects.get(pk=pk, workspace__slug=slug)

        if join_request.status != WorkspaceJoinRequest.Status.PENDING:
            return Response(
                {"error": "This request has already been responded to"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        workspace = join_request.workspace
        requester = join_request.requester

        # Create the membership, or reactivate a previously deactivated one.
        workspace_member = WorkspaceMember.objects.filter(workspace=workspace, member=requester).first()
        if workspace_member is not None:
            workspace_member.is_active = True
            workspace_member.role = join_request.role
            workspace_member.save()
        else:
            WorkspaceMember.objects.create(
                workspace=workspace,
                member=requester,
                role=join_request.role,
                created_by=request.user,
            )

        # Point the requester at the workspace they just joined so post-login
        # routing lands them there. ``last_workspace_id`` lives on Profile.
        Profile.objects.filter(user=requester).update(last_workspace_id=workspace.id)

        join_request.status = WorkspaceJoinRequest.Status.APPROVED
        join_request.responded_at = timezone.now()
        join_request.responded_by = request.user
        join_request.save()

        track_event.delay(
            user_id=requester.id,
            event_name=USER_JOINED_WORKSPACE,
            slug=slug,
            event_properties={
                "user_id": str(requester.id),
                "workspace_id": str(workspace.id),
                "workspace_slug": workspace.slug,
                "role": join_request.role,
                "joined_at": str(timezone.now()),
            },
        )

        return Response({"message": "Request approved"}, status=status.HTTP_200_OK)

    def deny(self, request, slug, pk):
        join_request = WorkspaceJoinRequest.objects.get(pk=pk, workspace__slug=slug)

        if join_request.status != WorkspaceJoinRequest.Status.PENDING:
            return Response(
                {"error": "This request has already been responded to"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        join_request.status = WorkspaceJoinRequest.Status.DENIED
        join_request.responded_at = timezone.now()
        join_request.responded_by = request.user
        join_request.save()

        return Response({"message": "Request denied"}, status=status.HTTP_200_OK)
