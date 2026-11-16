# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Project work-item types: list and create (NEWFRONT-984).

Types are workspace rows linked onto projects; the cloud type picker,
type filter and detail badge all read through the project list. Reads
are open to every active project member; creation is admin-only, like
labels. The routes ride the OSS stack API ungated, per the
NEWFRONT-672 precedent — the cloud gate lives in the web_new edition
module, not here.
"""

# Third Party imports
from rest_framework import status
from rest_framework.response import Response

# Module imports
from .. import BaseAPIView
from pi_dash.app.permissions import ProjectEntityPermission
from pi_dash.app.permissions import allow_permission, ROLE
from pi_dash.app.serializers import IssueTypeCreateSerializer, IssueTypeSerializer
from pi_dash.db.models import IssueType, Project
from pi_dash.db.models.issue_type import ProjectIssueType


class ProjectIssueTypesEndpoint(BaseAPIView):
    permission_classes = [ProjectEntityPermission]

    @allow_permission([ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST])
    def get(self, request, slug, project_id):
        project = Project.objects.filter(pk=project_id, workspace__slug=slug).first()
        if project is None:
            return Response({"error": "Project not found"}, status=status.HTTP_404_NOT_FOUND)
        types = (
            IssueType.objects.filter(
                project_issue_types__project_id=project_id,
                project_issue_types__deleted_at__isnull=True,
                deleted_at__isnull=True,
            )
            .order_by("project_issue_types__level", "name")
            .distinct()
        )
        return Response(IssueTypeSerializer(types, many=True).data, status=status.HTTP_200_OK)

    @allow_permission([ROLE.ADMIN])
    def post(self, request, slug, project_id):
        project = Project.objects.filter(pk=project_id, workspace__slug=slug).first()
        if project is None:
            return Response({"error": "Project not found"}, status=status.HTTP_404_NOT_FOUND)
        serializer = IssueTypeCreateSerializer(data=request.data)
        if not serializer.is_valid():
            return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)
        issue_type = serializer.save(workspace=project.workspace)
        level = ProjectIssueType.objects.filter(project_id=project_id, deleted_at__isnull=True).count()
        ProjectIssueType.objects.create(
            project_id=project_id,
            workspace_id=project.workspace_id,
            issue_type=issue_type,
            level=level,
        )
        return Response(IssueTypeSerializer(issue_type).data, status=status.HTTP_201_CREATED)
