# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Python imports
import json

# Django imports
from django.utils import timezone
from django.core.serializers.json import DjangoJSONEncoder

# Third Party imports
from rest_framework.response import Response
from rest_framework import status

# Module imports
from .. import BaseViewSet
from pi_dash.app.serializers import IssueWorklogSerializer
from pi_dash.app.permissions import allow_permission, ROLE
from pi_dash.db.models import (
    IntakeIssue,
    Issue,
    IssueAssignee,
    IssueWorklog,
    ProjectMember,
    WorkspaceMember,
)
from pi_dash.bgtasks.issue_activities_task import issue_activity
from pi_dash.utils.host import base_host


class IssueWorklogViewSet(BaseViewSet):
    """Per-issue time entries (ISS-206, cloud leg).

    Reads are open to every active project member; logging is for
    project admins and assignees on live, non-intake items (guests
    never); edits and removals are admin-or-author, mirroring comment
    authorship in the same feed. Every write records a
    ``worklog.activity.*`` row the feed reads through its own key.
    """

    serializer_class = IssueWorklogSerializer
    model = IssueWorklog

    def get_queryset(self):
        return (
            super()
            .get_queryset()
            .filter(workspace__slug=self.kwargs.get("slug"))
            .filter(project_id=self.kwargs.get("project_id"))
            .filter(issue_id=self.kwargs.get("issue_id"))
            .filter(
                project__project_projectmember__member=self.request.user,
                project__project_projectmember__is_active=True,
                project__archived_at__isnull=True,
            )
            .select_related("project")
            .select_related("workspace")
            .select_related("issue")
            .select_related("actor")
            .order_by("-created_at")
            .distinct()
        )

    def _can_log(self, slug, project_id, issue):
        membership = (
            ProjectMember.objects.filter(
                member=self.request.user,
                workspace__slug=slug,
                project_id=project_id,
                is_active=True,
            )
            .values_list("role", flat=True)
            .first()
        )
        if membership is None:
            return False
        is_admin = (
            membership == ROLE.ADMIN.value
            or WorkspaceMember.objects.filter(
                member=self.request.user,
                workspace__slug=slug,
                role=ROLE.ADMIN.value,
                is_active=True,
            ).exists()
        )
        if is_admin:
            return True
        return IssueAssignee.objects.filter(
            issue=issue,
            assignee=self.request.user,
            deleted_at__isnull=True,
        ).exists()

    @allow_permission([ROLE.ADMIN, ROLE.MEMBER])
    def create(self, request, slug, project_id, issue_id):
        issue = Issue.objects.get(pk=issue_id, project_id=project_id)
        if IntakeIssue.objects.filter(
            issue=issue,
            status__in=[-2, 0],
            workspace__slug=slug,
            project_id=project_id,
        ).exists():
            return Response(
                {"error": "Time cannot be logged on an intake item."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not self._can_log(slug, project_id, issue):
            return Response(
                {"error": "Only admins and assignees can log time on this work item."},
                status=status.HTTP_403_FORBIDDEN,
            )
        serializer = IssueWorklogSerializer(data=request.data)
        if serializer.is_valid():
            serializer.save(project_id=project_id, issue_id=issue_id, actor=request.user)
            issue_activity.delay(
                type="worklog.activity.created",
                requested_data=json.dumps(serializer.data, cls=DjangoJSONEncoder),
                actor_id=str(self.request.user.id),
                issue_id=str(self.kwargs.get("issue_id")),
                project_id=str(self.kwargs.get("project_id")),
                current_instance=None,
                epoch=int(timezone.now().timestamp()),
                notification=True,
                origin=base_host(request=request, is_app=True),
            )
            return Response(serializer.data, status=status.HTTP_201_CREATED)
        return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)

    @allow_permission(allowed_roles=[ROLE.ADMIN], creator=True, model=IssueWorklog)
    def partial_update(self, request, slug, project_id, issue_id, pk):
        worklog = IssueWorklog.objects.get(workspace__slug=slug, project_id=project_id, issue_id=issue_id, pk=pk)
        requested_data = json.dumps(self.request.data, cls=DjangoJSONEncoder)
        current_instance = json.dumps(IssueWorklogSerializer(worklog).data, cls=DjangoJSONEncoder)
        serializer = IssueWorklogSerializer(worklog, data=request.data, partial=True)
        if serializer.is_valid():
            serializer.save()
            issue_activity.delay(
                type="worklog.activity.updated",
                requested_data=requested_data,
                actor_id=str(request.user.id),
                issue_id=str(issue_id),
                project_id=str(project_id),
                current_instance=current_instance,
                epoch=int(timezone.now().timestamp()),
                notification=True,
                origin=base_host(request=request, is_app=True),
            )
            return Response(serializer.data, status=status.HTTP_200_OK)
        return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)

    @allow_permission(allowed_roles=[ROLE.ADMIN], creator=True, model=IssueWorklog)
    def destroy(self, request, slug, project_id, issue_id, pk):
        worklog = IssueWorklog.objects.get(workspace__slug=slug, project_id=project_id, issue_id=issue_id, pk=pk)
        current_instance = json.dumps(IssueWorklogSerializer(worklog).data, cls=DjangoJSONEncoder)
        worklog.delete()
        issue_activity.delay(
            type="worklog.activity.deleted",
            requested_data=json.dumps({"worklog_id": str(pk)}),
            actor_id=str(request.user.id),
            issue_id=str(issue_id),
            project_id=str(project_id),
            current_instance=current_instance,
            epoch=int(timezone.now().timestamp()),
            notification=True,
            origin=base_host(request=request, is_app=True),
        )
        return Response(status=status.HTTP_204_NO_CONTENT)
