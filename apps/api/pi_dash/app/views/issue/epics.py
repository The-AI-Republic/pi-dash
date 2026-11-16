# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Epic endpoints: the issues surface scoped to epic-typed rows (NEWFRONT-984).

There is no Epic model: an epic is an issue whose type carries
``is_epic``. These views subclass the issue views so epics inherit the
exact read/write/permission/activity behavior; each override only guards
the epic boundary (epic-typed rows in, everything else 404) or forces
the epic type on create. The routes ride the OSS stack API ungated, per
the NEWFRONT-672 precedent — the cloud gate lives in the web_new
edition module, not here.
"""

# Python imports
import json

# Django imports
from django.core.serializers.json import DjangoJSONEncoder
from django.http import QueryDict
from django.utils import timezone
from django.utils.decorators import method_decorator
from django.views.decorators.gzip import gzip_page

# Third Party imports
from rest_framework import status
from rest_framework.response import Response

# Module imports
from pi_dash.app.permissions import ROLE, allow_permission
from pi_dash.app.serializers import EpicIssueSerializer, IssueCreateSerializer
from pi_dash.bgtasks.issue_activities_task import issue_activity
from pi_dash.bgtasks.issue_description_version_task import issue_description_version_task
from pi_dash.bgtasks.webhook_task import model_activity
from pi_dash.db.models import Issue, Project
from pi_dash.db.models.issue_type import ProjectIssueType
from pi_dash.utils.grouper import issue_queryset_grouper
from pi_dash.utils.host import base_host
from pi_dash.utils.timezone_converter import user_timezone_converter

# Sibling views (subclassed, never copied)
from .base import IssueViewSet
from .sub_issue import SubIssuesEndpoint


def resolve_epic_type(project_id):
    """The project's epic type, or None when it links none."""
    link = (
        ProjectIssueType.objects.filter(
            project_id=project_id,
            deleted_at__isnull=True,
            issue_type__is_epic=True,
            issue_type__is_active=True,
            issue_type__deleted_at__isnull=True,
        )
        .select_related("issue_type")
        .order_by("level")
        .first()
    )
    return link.issue_type if link is not None else None


class EpicViewSet(IssueViewSet):
    def get_queryset(self):
        return super().get_queryset().filter(type__is_epic=True)

    def get_serializer_class(self):
        if self.action in ["create", "update", "partial_update"]:
            return IssueCreateSerializer
        return EpicIssueSerializer

    @allow_permission([ROLE.ADMIN, ROLE.MEMBER])
    def create(self, request, slug, project_id):
        epic_type = resolve_epic_type(project_id)
        if epic_type is None:
            return Response(
                {"error": "This project has no epic type, so no epic can be created."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        project = Project.objects.get(pk=project_id)
        data = request.data.copy() if isinstance(request.data, QueryDict) else dict(request.data)
        # The epics collection only holds epics: the type is forced, never
        # taken from the caller (the create schema carries no type field).
        data["type"] = str(epic_type.id)

        serializer = IssueCreateSerializer(
            data=data,
            context={
                "request": request,
                "project_id": project_id,
                "workspace_id": project.workspace_id,
                "default_assignee_id": project.default_assignee_id,
            },
        )

        if serializer.is_valid():
            serializer.save()

            # Track the issue
            issue_activity.delay(
                type="issue.activity.created",
                requested_data=json.dumps(data, cls=DjangoJSONEncoder),
                actor_id=str(request.user.id),
                issue_id=str(serializer.data.get("id", None)),
                project_id=str(project_id),
                current_instance=None,
                epoch=int(timezone.now().timestamp()),
                notification=True,
                origin=base_host(request=request, is_app=True),
            )
            queryset = self.get_queryset()
            queryset = self.apply_annotations(queryset)
            issue = (
                issue_queryset_grouper(
                    queryset=queryset.filter(pk=serializer.data["id"]),
                    group_by=None,
                    sub_group_by=None,
                )
                .values(
                    "id",
                    "name",
                    "state_id",
                    "sort_order",
                    "completed_at",
                    "estimate_point",
                    "priority",
                    "start_date",
                    "target_date",
                    "sequence_id",
                    "project_id",
                    "parent_id",
                    "cycle_id",
                    "module_ids",
                    "label_ids",
                    "assignee_ids",
                    "sub_issues_count",
                    "created_at",
                    "updated_at",
                    "created_by",
                    "updated_by",
                    "attachment_count",
                    "link_count",
                    "is_draft",
                    "archived_at",
                    "deleted_at",
                )
                .first()
            )
            datetime_fields = ["created_at", "updated_at"]
            issue = user_timezone_converter(issue, datetime_fields, request.user.user_timezone)
            # Send the model activity
            model_activity.delay(
                model_name="issue",
                model_id=str(serializer.data["id"]),
                requested_data=data,
                current_instance=None,
                actor_id=request.user.id,
                slug=slug,
                origin=base_host(request=request, is_app=True),
            )
            # updated issue description version
            issue_description_version_task.delay(
                updated_issue=json.dumps(data, cls=DjangoJSONEncoder),
                issue_id=str(serializer.data["id"]),
                user_id=request.user.id,
                is_creating=True,
            )
            return Response(issue, status=status.HTTP_201_CREATED)
        return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)

    @allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST], creator=True, model=Issue)
    def retrieve(self, request, slug, project_id, pk=None):
        if self.get_queryset().filter(pk=pk).first() is None:
            return Response(
                {"error": "The required object does not exist."},
                status=status.HTTP_404_NOT_FOUND,
            )
        # Keyword form: the parent's permission decorator reads the slug
        # back out of the call kwargs.
        response = super().retrieve(request, slug=slug, project_id=project_id, pk=pk)
        if response.status_code == status.HTTP_200_OK:
            response.data["is_epic"] = True
        return response

    @allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER], creator=True, model=Issue)
    def partial_update(self, request, slug, project_id, pk=None):
        if request.data is not None and "type" in request.data:
            return Response(
                {"error": "An epic's type cannot change through the epics collection."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        return super().partial_update(request, slug=slug, project_id=project_id, pk=pk)

    @allow_permission([ROLE.ADMIN], creator=True, model=Issue)
    def destroy(self, request, slug, project_id, pk=None):
        if self.get_queryset().filter(pk=pk).first() is None:
            return Response(
                {"error": "The required object does not exist."},
                status=status.HTTP_404_NOT_FOUND,
            )
        return super().destroy(request, slug=slug, project_id=project_id, pk=pk)


class EpicWorkItemsEndpoint(SubIssuesEndpoint):
    """An epic's children: the sub-issues collection with an epic parent."""

    def _epic_or_none(self, slug, project_id, epic_id):
        return Issue.issue_objects.filter(
            pk=epic_id,
            project_id=project_id,
            workspace__slug=slug,
            type__is_epic=True,
        ).first()

    @method_decorator(gzip_page)
    def get(self, request, slug, project_id, epic_id):
        if self._epic_or_none(slug, project_id, epic_id) is None:
            return Response(
                {"error": "The required object does not exist."},
                status=status.HTTP_404_NOT_FOUND,
            )
        return super().get(request, slug=slug, project_id=project_id, issue_id=epic_id)

    # Assign multiple work items to the epic
    def post(self, request, slug, project_id, epic_id):
        if self._epic_or_none(slug, project_id, epic_id) is None:
            return Response(
                {"error": "The required object does not exist."},
                status=status.HTTP_404_NOT_FOUND,
            )
        return super().post(request, slug=slug, project_id=project_id, issue_id=epic_id)
