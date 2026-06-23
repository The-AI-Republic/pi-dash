# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Session app-API endpoints for provider-neutral code-review links."""

from rest_framework import status
from rest_framework.response import Response

from .. import BaseViewSet
from pi_dash.app.permissions import ProjectEntityPermission
from pi_dash.app.serializers import GitCodeReviewLinkSerializer
from pi_dash.db.models import GitCodeReviewLink
from pi_dash.integrations.git.adapters.base import GitProviderNotFoundError
from pi_dash.integrations.git.code_reviews import (
    CodeReviewAlreadyLinked,
    InvalidCodeReviewURL,
    IssueNotFound,
    attach_code_review,
)


class GitCodeReviewLinkViewSet(BaseViewSet):
    permission_classes = [ProjectEntityPermission]

    model = GitCodeReviewLink
    serializer_class = GitCodeReviewLinkSerializer

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

    def destroy(self, request, slug, project_id, issue_id, pk):
        link = self.get_queryset().get(pk=pk)
        link.delete()
        return Response(status=status.HTTP_204_NO_CONTENT)
