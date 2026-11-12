# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from unittest.mock import patch

import pytest
from rest_framework import status

from pi_dash.authentication.utils.workspace_project_join import process_workspace_project_invitations
from pi_dash.db.models import ProjectMember, ProjectMemberInvite, User, WorkspaceMember


def get_invite_url(workspace_slug: str, project_id) -> str:
    return f"/api/workspaces/{workspace_slug}/projects/{project_id}/invitations/"


@pytest.fixture
def project_admin(project, create_user):
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)


@pytest.mark.contract
class TestProjectInvitationsCreate:
    """POST /api/workspaces/<slug>/projects/<project_id>/invitations/"""

    @pytest.mark.django_db
    def test_invite_workspace_guest_with_matching_role(self, session_client, workspace, project, project_admin):
        guest = User.objects.create_user(email="guest@example.com", username="guest")
        WorkspaceMember.objects.create(workspace=workspace, member=guest, role=5)

        with patch("pi_dash.app.views.project.invite.project_invitation") as task:
            response = session_client.post(
                get_invite_url(workspace.slug, project.id),
                {"emails": [{"email": "guest@example.com", "role": 5}]},
                format="json",
            )

        assert response.status_code == status.HTTP_200_OK
        invite = ProjectMemberInvite.objects.get(project=project, email="guest@example.com")
        assert invite.role == 5
        task.delay.assert_called_once()
        assert task.delay.call_args.args[0] == "guest@example.com"

    @pytest.mark.django_db
    def test_invite_non_workspace_member(self, session_client, workspace, project, project_admin):
        with patch("pi_dash.app.views.project.invite.project_invitation") as task:
            response = session_client.post(
                get_invite_url(workspace.slug, project.id),
                {"emails": [{"email": "outsider@example.com", "role": 15}]},
                format="json",
            )

        assert response.status_code == status.HTTP_200_OK
        assert ProjectMemberInvite.objects.filter(project=project, email="outsider@example.com").exists()
        task.delay.assert_called_once()

    @pytest.mark.django_db
    def test_invite_workspace_guest_with_different_role_is_rejected(
        self, session_client, workspace, project, project_admin
    ):
        guest = User.objects.create_user(email="guest@example.com", username="guest")
        WorkspaceMember.objects.create(workspace=workspace, member=guest, role=5)

        with patch("pi_dash.app.views.project.invite.project_invitation") as task:
            response = session_client.post(
                get_invite_url(workspace.slug, project.id),
                {"emails": [{"email": "guest@example.com", "role": 15}]},
                format="json",
            )

        assert response.data == {"error": "You cannot invite a user with different role than workspace role"}
        assert not ProjectMemberInvite.objects.exists()
        task.delay.assert_not_called()


@pytest.mark.contract
class TestAcceptedProjectInviteJoin:
    """process_workspace_project_invitations consumes accepted project invites (NEWFRONT-151)."""

    @pytest.fixture
    def invited_guest(self, workspace, project):
        guest = User.objects.create_user(email="invited-guest@example.com", username="invited-guest")
        ProjectMemberInvite.objects.create(
            project=project,
            workspace=workspace,
            email=guest.email,
            token="test-token",
            role=5,
            accepted=True,
        )
        return guest

    @pytest.mark.django_db
    def test_accepted_invite_creates_membership_and_consumes_invite(self, workspace, project, invited_guest):
        process_workspace_project_invitations(invited_guest)

        membership = ProjectMember.objects.get(project=project, member=invited_guest)
        assert membership.workspace_id == workspace.id
        assert membership.role == 5
        assert WorkspaceMember.objects.filter(workspace=workspace, member=invited_guest).exists()
        assert not ProjectMemberInvite.objects.filter(project=project, email=invited_guest.email).exists()

    @pytest.mark.django_db
    def test_repeat_join_is_idempotent(self, workspace, project, invited_guest):
        process_workspace_project_invitations(invited_guest)
        process_workspace_project_invitations(invited_guest)

        assert ProjectMember.objects.filter(project=project, member=invited_guest).count() == 1
        assert not ProjectMemberInvite.objects.filter(project=project, email=invited_guest.email).exists()
