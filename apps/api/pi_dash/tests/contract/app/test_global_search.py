# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import Issue, IssueComment, ProjectMember


def _search_url(slug):
    return f"/api/workspaces/{slug}/search/"


def _make_issue(project, user, *, name, description=""):
    return Issue.objects.create(
        name=name,
        description_html=f"<p>{description}</p>" if description else "",
        description_stripped=description,
        project=project,
        workspace=project.workspace,
        created_by=user,
    )


@pytest.fixture
def searchable_project(project, create_user):
    ProjectMember.objects.get_or_create(
        project=project,
        member=create_user,
        defaults={"role": 20, "is_active": True},
    )
    return project


@pytest.mark.contract
class TestGlobalSearchIssueMatches:
    @pytest.mark.django_db
    def test_issue_body_match_surfaces_issue(self, session_client, workspace, searchable_project, create_user):
        issue = _make_issue(
            searchable_project,
            create_user,
            name="unrelated title",
            description="browser console has 38 same errors",
        )

        response = session_client.get(
            _search_url(workspace.slug),
            {"search": "38 same errors", "workspace_search": "true", "entities": "issue"},
        )

        assert response.status_code == http_status.HTTP_200_OK
        ids = [str(result["id"]) for result in response.data["results"]["issue"]]
        assert str(issue.id) in ids

    @pytest.mark.django_db
    def test_issue_comment_match_surfaces_parent_issue(
        self, session_client, workspace, searchable_project, create_user
    ):
        issue = _make_issue(
            searchable_project,
            create_user,
            name="unrelated title",
            description="unrelated description",
        )
        IssueComment.objects.create(
            issue=issue,
            project=searchable_project,
            workspace=searchable_project.workspace,
            comment_html="<p>release blocker from upload retry loop</p>",
            comment_stripped="release blocker from upload retry loop",
            actor=create_user,
        )

        response = session_client.get(
            _search_url(workspace.slug),
            {"search": "upload retry loop", "workspace_search": "true", "entities": "issue"},
        )

        assert response.status_code == http_status.HTTP_200_OK
        ids = [str(result["id"]) for result in response.data["results"]["issue"]]
        assert str(issue.id) in ids
