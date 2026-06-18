# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Session app-API endpoints for the issue-detail "Pull requests" overview.

Mirrors the external-API ``pidash issue attach-pr`` contract (which the coding
agent uses) but with session auth for the web UI. Both surfaces share
``utils.github_pr_links`` so attach behaviour is identical.
"""

from rest_framework import status
from rest_framework.response import Response

from .. import BaseViewSet
from pi_dash.app.permissions import ProjectEntityPermission
from pi_dash.app.serializers import GithubPullRequestLinkSerializer
from pi_dash.db.models import GithubPullRequestLink
from pi_dash.utils.github_pr_links import (
    InvalidPullRequestURL,
    PullRequestAlreadyLinked,
    attach_pull_request,
)


class GithubPullRequestLinkViewSet(BaseViewSet):
    permission_classes = [ProjectEntityPermission]

    model = GithubPullRequestLink
    serializer_class = GithubPullRequestLinkSerializer

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
            .order_by("-created_at")
            .distinct()
        )

    def create(self, request, slug, project_id, issue_id):
        try:
            link, created = attach_pull_request(
                project_id=project_id, issue_id=issue_id, workspace_slug=slug, raw_url=request.data.get("url"),
            )
        except InvalidPullRequestURL:
            return Response(
                {"error": "A valid github.com pull request URL is required."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        except PullRequestAlreadyLinked as e:
            return Response({"error": str(e)}, status=status.HTTP_409_CONFLICT)

        return Response(
            GithubPullRequestLinkSerializer(link).data,
            status=status.HTTP_201_CREATED if created else status.HTTP_200_OK,
        )

    def destroy(self, request, slug, project_id, issue_id, pk):
        link = self.get_queryset().get(pk=pk)
        link.delete()
        return Response(status=status.HTTP_204_NO_CONTENT)
