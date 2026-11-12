# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for project-member role writes (NEWFRONT-1037, PDASHOSS01-328).

PATCH ``/api/workspaces/<slug>/projects/<pid>/members/<mid>/`` with a
``role`` field: a workspace admin may set any project role for anyone who
is not a workspace admin (including promoting to project Admin) and may
change their own role; nobody may change another workspace admin's role
(403); everyone else keeps the pre-existing guards.
"""

import pytest
from rest_framework import status

from pi_dash.db.models import ProjectMember, User, WorkspaceMember


def _member_url(slug, project_id, membership_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/members/{membership_id}/"


@pytest.fixture
def member_user(db):
    return User.objects.create_user(email="member@example.com", username="member")


@pytest.fixture
def project_admin_user(db):
    return User.objects.create_user(email="project-admin@example.com", username="project-admin")


@pytest.fixture
def ws_admin_member_user(db):
    return User.objects.create_user(email="ws-admin-member@example.com", username="ws-admin-member")


@pytest.fixture
def ws_admin_owner_user(db):
    return User.objects.create_user(email="ws-admin-owner@example.com", username="ws-admin-owner")


@pytest.fixture
def guest_user(db):
    return User.objects.create_user(email="guest@example.com", username="guest")


@pytest.fixture
def role_matrix(
    workspace,
    project,
    create_user,
    member_user,
    project_admin_user,
    ws_admin_member_user,
    ws_admin_owner_user,
    guest_user,
):
    """Seed one project membership per standing. ``create_user`` is already a
    workspace admin (role 20) via the ``workspace`` fixture."""
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=member_user, role=15)
    ProjectMember.objects.create(project=project, member=member_user, role=15, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=project_admin_user, role=15)
    ProjectMember.objects.create(project=project, member=project_admin_user, role=20, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=ws_admin_member_user, role=20)
    ProjectMember.objects.create(project=project, member=ws_admin_member_user, role=15, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=ws_admin_owner_user, role=20)
    ProjectMember.objects.create(project=project, member=ws_admin_owner_user, role=20, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=guest_user, role=5)
    ProjectMember.objects.create(project=project, member=guest_user, role=5, is_active=True)
    return project


def _patch_role(api_client, user, url, role):
    api_client.force_authenticate(user=user)
    return api_client.patch(url, {"role": role}, format="json")


def _project_role(project, user):
    return ProjectMember.objects.get(project=project, member=user).role


@pytest.mark.contract
class TestWorkspaceAdminTargetProtected:
    """Nobody may change another workspace admin's project role (403, unchanged)."""

    @pytest.mark.django_db
    def test_plain_member_cannot_change_workspace_admin_role(
        self, api_client, workspace, role_matrix, member_user, ws_admin_owner_user
    ):
        target = ProjectMember.objects.get(project=role_matrix, member=ws_admin_owner_user)
        response = _patch_role(api_client, member_user, _member_url(workspace.slug, role_matrix.id, target.id), 15)

        assert response.status_code == status.HTTP_403_FORBIDDEN
        assert response.data == {"error": "You cannot update the role of a workspace admin"}
        assert _project_role(role_matrix, ws_admin_owner_user) == 20

    @pytest.mark.django_db
    def test_project_admin_cannot_change_workspace_admin_who_is_only_a_project_member(
        self, api_client, workspace, role_matrix, project_admin_user, ws_admin_member_user
    ):
        target = ProjectMember.objects.get(project=role_matrix, member=ws_admin_member_user)
        response = _patch_role(
            api_client, project_admin_user, _member_url(workspace.slug, role_matrix.id, target.id), 5
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN
        assert response.data == {"error": "You cannot update the role of a workspace admin"}
        assert _project_role(role_matrix, ws_admin_member_user) == 15

    @pytest.mark.django_db
    def test_workspace_admin_cannot_change_another_workspace_admin_role(
        self, api_client, workspace, role_matrix, create_user, ws_admin_member_user
    ):
        target = ProjectMember.objects.get(project=role_matrix, member=ws_admin_member_user)
        response = _patch_role(api_client, create_user, _member_url(workspace.slug, role_matrix.id, target.id), 5)

        assert response.status_code == status.HTTP_403_FORBIDDEN
        assert response.data == {"error": "You cannot update the role of a workspace admin"}
        assert _project_role(role_matrix, ws_admin_member_user) == 15


@pytest.mark.contract
class TestWorkspaceAdminBypass:
    """A workspace admin sets any project role for non-workspace-admins and
    may change their own role."""

    @pytest.mark.django_db
    def test_workspace_admin_who_is_only_a_project_member_can_change_non_admin_role(
        self, api_client, workspace, role_matrix, ws_admin_member_user, member_user
    ):
        target = ProjectMember.objects.get(project=role_matrix, member=member_user)
        response = _patch_role(
            api_client, ws_admin_member_user, _member_url(workspace.slug, role_matrix.id, target.id), 5
        )

        assert response.status_code == status.HTTP_200_OK
        assert _project_role(role_matrix, member_user) == 5

    @pytest.mark.django_db
    def test_workspace_admin_who_is_only_a_project_member_can_promote_to_project_admin(
        self, api_client, workspace, role_matrix, ws_admin_member_user, member_user
    ):
        target = ProjectMember.objects.get(project=role_matrix, member=member_user)
        response = _patch_role(
            api_client, ws_admin_member_user, _member_url(workspace.slug, role_matrix.id, target.id), 20
        )

        assert response.status_code == status.HTTP_200_OK
        assert _project_role(role_matrix, member_user) == 20

    @pytest.mark.django_db
    def test_workspace_admin_can_change_own_role(self, api_client, workspace, role_matrix, create_user):
        target = ProjectMember.objects.get(project=role_matrix, member=create_user)
        response = _patch_role(api_client, create_user, _member_url(workspace.slug, role_matrix.id, target.id), 15)

        assert response.status_code == status.HTTP_200_OK
        assert _project_role(role_matrix, create_user) == 15


@pytest.mark.contract
class TestExistingGuardsKept:
    """Everyone else keeps today's guards: the admin-only, equal-or-higher,
    own-role and workspace-guest rules are unchanged."""

    @pytest.mark.django_db
    def test_project_admin_cannot_promote_member_to_admin(
        self, api_client, workspace, role_matrix, project_admin_user, member_user
    ):
        target = ProjectMember.objects.get(project=role_matrix, member=member_user)
        response = _patch_role(
            api_client, project_admin_user, _member_url(workspace.slug, role_matrix.id, target.id), 20
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN
        assert response.data == {"error": "You cannot assign a role equal to or higher than your own"}
        assert _project_role(role_matrix, member_user) == 15

    @pytest.mark.django_db
    def test_project_admin_can_demote_member(self, api_client, workspace, role_matrix, project_admin_user, member_user):
        target = ProjectMember.objects.get(project=role_matrix, member=member_user)
        response = _patch_role(
            api_client, project_admin_user, _member_url(workspace.slug, role_matrix.id, target.id), 5
        )

        assert response.status_code == status.HTTP_200_OK
        assert _project_role(role_matrix, member_user) == 5

    @pytest.mark.django_db
    def test_plain_member_cannot_change_roles(self, api_client, workspace, role_matrix, member_user, guest_user):
        target = ProjectMember.objects.get(project=role_matrix, member=guest_user)
        response = _patch_role(api_client, member_user, _member_url(workspace.slug, role_matrix.id, target.id), 15)

        assert response.status_code == status.HTTP_403_FORBIDDEN
        assert response.data == {"error": "You do not have permission to update roles"}
        assert _project_role(role_matrix, guest_user) == 5

    @pytest.mark.django_db
    def test_non_admin_cannot_update_own_role(self, api_client, workspace, role_matrix, member_user):
        target = ProjectMember.objects.get(project=role_matrix, member=member_user)
        response = _patch_role(api_client, member_user, _member_url(workspace.slug, role_matrix.id, target.id), 5)

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "You cannot update your own role"}
        assert _project_role(role_matrix, member_user) == 15

    @pytest.mark.django_db
    @pytest.mark.parametrize("requester", ["create_user", "project_admin_user", "ws_admin_member_user"])
    def test_workspace_guest_cap_applies_to_every_requester(
        self, api_client, workspace, role_matrix, guest_user, requester, request
    ):
        requester_user = request.getfixturevalue(requester)
        target = ProjectMember.objects.get(project=role_matrix, member=guest_user)
        response = _patch_role(api_client, requester_user, _member_url(workspace.slug, role_matrix.id, target.id), 15)

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "You cannot add a user with role higher than the workspace role"}
        assert _project_role(role_matrix, guest_user) == 5
