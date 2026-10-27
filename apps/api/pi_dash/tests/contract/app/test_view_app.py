# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for project view retrieve (VIEW-022, NEWFRONT-227).

An unknown project view id must miss with 404. A view that exists but is
private to another user and hidden from guests must keep failing closed with
403, rather than being conflated with a missing row.
"""

from unittest.mock import patch
from uuid import uuid4

import pytest
from rest_framework import status

from pi_dash.db.models import IssueView, ProjectMember, User, WorkspaceMember


def _view_url(slug, project_id, view_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/views/{view_id}/"


@pytest.fixture
def member_membership(workspace, project, create_user):
    return ProjectMember.objects.create(
        project=project,
        member=create_user,
        role=20,
        is_active=True,
    )


@pytest.fixture
def project_view(workspace, project, create_user, member_membership):
    return IssueView.objects.create(
        name="Active issues",
        description="Things in flight",
        filters={},
        project=project,
        workspace=workspace,
        owned_by=create_user,
    )


@pytest.mark.contract
@pytest.mark.django_db
class TestProjectViewRetrieve:
    @patch("pi_dash.app.views.view.base.recent_visited_task")
    def test_unknown_view_id_returns_404(self, recent_visited, session_client, workspace, project, member_membership):
        response = session_client.get(_view_url(workspace.slug, project.id, uuid4()))

        assert response.status_code == status.HTTP_404_NOT_FOUND
        assert response.data == {"error": "View not found"}
        recent_visited.delay.assert_not_called()

    @patch("pi_dash.app.views.view.base.recent_visited_task")
    def test_existing_view_id_returns_record(self, recent_visited, session_client, workspace, project, project_view):
        response = session_client.get(_view_url(workspace.slug, project.id, project_view.id))

        assert response.status_code == status.HTTP_200_OK
        assert str(response.data["id"]) == str(project_view.id)
        assert response.data["name"] == "Active issues"
        recent_visited.delay.assert_called_once()

    @patch("pi_dash.app.views.view.base.recent_visited_task")
    def test_guest_cannot_retrieve_private_view(
        self, recent_visited, api_client, workspace, project, create_user, project_view
    ):
        guest = User.objects.create_user(email="view-guest@example.com", username="view-guest")
        WorkspaceMember.objects.create(workspace=workspace, member=guest, role=5)
        ProjectMember.objects.create(project=project, member=guest, role=5, is_active=True)
        api_client.force_authenticate(user=guest)

        response = api_client.get(_view_url(workspace.slug, project.id, project_view.id))

        assert response.status_code == status.HTTP_403_FORBIDDEN
        recent_visited.delay.assert_not_called()
