# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Python imports
from itertools import chain

# Django imports
from django.db.models import Prefetch, Q
from django.utils.decorators import method_decorator
from django.views.decorators.gzip import gzip_page

# Third Party imports
from rest_framework.response import Response
from rest_framework import status

# Module imports
from .. import BaseAPIView
from pi_dash.app.serializers import IssueActivitySerializer, IssueCommentSerializer
from pi_dash.app.permissions import ProjectEntityPermission, allow_permission, ROLE
from pi_dash.db.models import IssueActivity, IssueComment, CommentReaction, IntakeIssue


class IssueActivityEndpoint(BaseAPIView):
    permission_classes = [ProjectEntityPermission]
    use_read_replica = True

    @method_decorator(gzip_page)
    @allow_permission([ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST])
    def get(self, request, slug, project_id, issue_id):
        filters = {}
        if request.GET.get("created_at__gt", None) is not None:
            filters = {"created_at__gt": request.GET.get("created_at__gt")}

        issue_activities = (
            IssueActivity.objects.filter(issue_id=issue_id)
            .filter(
                ~Q(field__in=["comment", "vote", "reaction", "draft"]),
                project__project_projectmember__member=self.request.user,
                project__project_projectmember__is_active=True,
                project__archived_at__isnull=True,
                workspace__slug=slug,
            )
            .filter(**filters)
            .select_related("actor", "workspace", "issue", "project")
        ).order_by("created_at")
        issue_comments = (
            IssueComment.objects.filter(issue_id=issue_id)
            .filter(
                project__project_projectmember__member=self.request.user,
                project__project_projectmember__is_active=True,
                project__archived_at__isnull=True,
                workspace__slug=slug,
            )
            .filter(**filters)
            .order_by("created_at")
            .select_related("actor", "issue", "project", "workspace")
            .prefetch_related(
                Prefetch(
                    "comment_reactions",
                    queryset=CommentReaction.objects.select_related("actor"),
                )
            )
        )

        # The property rows carry the intake source only when the related
        # intake is prefetched onto ``issue.source_data``; do it for every
        # branch so the merged and split reads describe a row the same way.
        issue_activities = issue_activities.prefetch_related(
            Prefetch(
                "issue__issue_intake",
                queryset=IntakeIssue.objects.only("source_email", "source", "extra"),
                to_attr="source_data",
            )
        )

        activity_type = request.GET.get("activity_type", None)

        if activity_type == "issue-property":
            return Response(
                IssueActivitySerializer(issue_activities, many=True).data,
                status=status.HTTP_200_OK,
            )

        if activity_type == "issue-comment":
            return Response(
                IssueCommentSerializer(issue_comments, many=True).data,
                status=status.HTTP_200_OK,
            )

        # Merged read: serialize each side with its own serializer first, then
        # merge the plain dicts. Sorting the model rows directly would compare
        # model instances and blow up on the subscript.
        result_list = sorted(
            chain(
                IssueActivitySerializer(issue_activities, many=True).data,
                IssueCommentSerializer(issue_comments, many=True).data,
            ),
            key=lambda entry: entry["created_at"],
        )

        return Response(result_list, status=status.HTTP_200_OK)
