# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""External-API endpoints for linking a work item to GitHub pull requests.

This is the server-side contract behind the ``pidash issue attach-pr`` CLI
command. Attaching/detaching never mutates the issue's workflow state; the
PR status snapshot is best-effort at attach time and refreshed by the GitHub
App ``pull_request`` webhook (see ``app/views/integration/github.py``).
"""

from rest_framework import status
from rest_framework.response import Response

from pi_dash.api.serializers import GithubPullRequestLinkSerializer
from pi_dash.api.views.base import BaseAPIView
from pi_dash.app.permissions import ProjectEntityPermission
from pi_dash.db.models import GithubPullRequestLink
from pi_dash.utils.github_pr_links import (
    InvalidPullRequestURL,
    IssueNotFound,
    PullRequestAlreadyLinked,
    attach_pull_request,
)


class GithubPullRequestLinkListCreateAPIEndpoint(BaseAPIView):
    """List PRs linked to a work item, or attach a new one by URL."""

    model = GithubPullRequestLink
    serializer_class = GithubPullRequestLinkSerializer
    permission_classes = [ProjectEntityPermission]
    use_read_replica = True

    def get_queryset(self):
        return (
            GithubPullRequestLink.objects.filter(workspace__slug=self.kwargs.get("slug"))
            .filter(project_id=self.kwargs.get("project_id"))
            .filter(issue_id=self.kwargs.get("issue_id"))
            .filter(
                project__project_projectmember__member=self.request.user,
                project__project_projectmember__is_active=True,
            )
            .filter(project__archived_at__isnull=True)
            .order_by(self.kwargs.get("order_by", "-created_at"))
            .distinct()
        )

    def get(self, request, slug, project_id, issue_id):
        return self.paginate(
            request=request,
            queryset=self.get_queryset(),
            on_results=lambda links: GithubPullRequestLinkSerializer(links, many=True).data,
        )

    def post(self, request, slug, project_id, issue_id):
        try:
            link, created = attach_pull_request(
                project_id=project_id, issue_id=issue_id, workspace_slug=slug, raw_url=request.data.get("url"),
            )
        except InvalidPullRequestURL:
            return Response(
                {"error": "A valid github.com pull request URL is required."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        except IssueNotFound:
            return Response({"error": "Work item not found."}, status=status.HTTP_404_NOT_FOUND)
        except PullRequestAlreadyLinked as e:
            return Response({"error": str(e)}, status=status.HTTP_409_CONFLICT)

        return Response(
            GithubPullRequestLinkSerializer(link).data,
            status=status.HTTP_201_CREATED if created else status.HTTP_200_OK,
        )


class GithubPullRequestLinkDetailAPIEndpoint(BaseAPIView):
    """Detach (soft-delete) a linked PR."""

    model = GithubPullRequestLink
    serializer_class = GithubPullRequestLinkSerializer
    permission_classes = [ProjectEntityPermission]

    def get_queryset(self):
        return (
            GithubPullRequestLink.objects.filter(workspace__slug=self.kwargs.get("slug"))
            .filter(project_id=self.kwargs.get("project_id"))
            .filter(issue_id=self.kwargs.get("issue_id"))
            .filter(
                project__project_projectmember__member=self.request.user,
                project__project_projectmember__is_active=True,
            )
        )

    def delete(self, request, slug, project_id, issue_id, pk):
        link = self.get_queryset().get(pk=pk)
        link.delete()
        return Response(status=status.HTTP_204_NO_CONTENT)
