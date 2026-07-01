# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""External-API endpoints for provider-neutral code-review links."""

from rest_framework import status
from rest_framework.response import Response

from pi_dash.api.serializers import GitCodeReviewLinkSerializer
from pi_dash.api.views.base import BaseAPIView
from pi_dash.app.permissions import ProjectEntityPermission
from pi_dash.db.models import GitCodeReviewLink
from pi_dash.integrations.git.adapters.base import GitProviderNotFoundError
from pi_dash.integrations.git.code_reviews import (
    CodeReviewAlreadyLinked,
    InvalidCodeReviewURL,
    IssueNotFound,
    attach_code_review,
    detach_code_review_link,
)


class GitCodeReviewLinkListCreateAPIEndpoint(BaseAPIView):
    """List provider-neutral code reviews linked to a work item, or attach one by URL."""

    model = GitCodeReviewLink
    serializer_class = GitCodeReviewLinkSerializer
    permission_classes = [ProjectEntityPermission]
    use_read_replica = True

    def get_queryset(self):
        return (
            GitCodeReviewLink.objects.filter(workspace__slug=self.kwargs.get("slug"))
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
            on_results=lambda links: GitCodeReviewLinkSerializer(links, many=True).data,
        )

    def post(self, request, slug, project_id, issue_id):
        try:
            link, created = attach_code_review(
                project_id=project_id,
                issue_id=issue_id,
                workspace_slug=slug,
                raw_url=request.data.get("url"),
            )
        except InvalidCodeReviewURL:
            return Response(
                {"error": "A supported GitHub pull request or GitLab merge request URL is required."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        except IssueNotFound:
            return Response({"error": "Work item not found."}, status=status.HTTP_404_NOT_FOUND)
        except CodeReviewAlreadyLinked as e:
            return Response({"error": str(e)}, status=status.HTTP_409_CONFLICT)
        except GitProviderNotFoundError:
            return Response({"error": "Code review not found."}, status=status.HTTP_404_NOT_FOUND)

        return Response(
            GitCodeReviewLinkSerializer(link).data,
            status=status.HTTP_201_CREATED if created else status.HTTP_200_OK,
        )


class GitCodeReviewLinkDetailAPIEndpoint(BaseAPIView):
    """Detach a linked provider-neutral code review."""

    model = GitCodeReviewLink
    serializer_class = GitCodeReviewLinkSerializer
    permission_classes = [ProjectEntityPermission]

    def get_queryset(self):
        return (
            GitCodeReviewLink.objects.filter(workspace__slug=self.kwargs.get("slug"))
            .filter(project_id=self.kwargs.get("project_id"))
            .filter(issue_id=self.kwargs.get("issue_id"))
            .filter(
                project__project_projectmember__member=self.request.user,
                project__project_projectmember__is_active=True,
            )
        )

    def delete(self, request, slug, project_id, issue_id, pk):
        link = self.get_queryset().get(pk=pk)
        detach_code_review_link(link)
        return Response(status=status.HTTP_204_NO_CONTENT)
