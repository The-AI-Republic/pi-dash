# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Who may edit a project membership through fields other than ``role`` (PDASHOSS01-329).

Any field on someone else's membership needs a project Admin or a workspace
admin, and nobody can change their own ``is_active`` here. The ``role``
rules live in ``test_project_member_role_app.py``.
"""

import pytest
from rest_framework import status as http_status
from rest_framework.test import APIClient

from pi_dash.db.models import ProjectMember, User, WorkspaceMember

ADMIN, MEMBER, GUEST = 20, 15, 5

OWN_ROLE = {"error": "You cannot update your own role"}
NOT_MEMBER_EDITOR = {"error": "You do not have permission to update this member"}
OWN_STATUS = {"error": "You cannot update your own membership status"}


@pytest.fixture
def member_of(project):
    def _member(name, workspace_role, project_role):
        email = f"{name}@example.com"
        user = User.objects.create(email=email, username=name, first_name=name)
        WorkspaceMember.objects.create(workspace=project.workspace, member=user, role=workspace_role, is_active=True)
        membership = ProjectMember.objects.create(project=project, member=user, role=project_role, is_active=True)
        client = APIClient()
        client.force_authenticate(user=user)
        return client, membership

    return _member


@pytest.fixture
def patch_member(project):
    def _patch(client, membership, data):
        return client.patch(
            f"/api/workspaces/{project.workspace.slug}/projects/{project.id}/members/{membership.id}/",
            data,
            format="json",
        )

    return _patch


NON_ROLE_EDITS = [
    pytest.param({"comment": "x"}, id="comment"),
    pytest.param({"is_active": False}, id="deactivate"),
]


def snapshot(membership):
    membership.refresh_from_db()
    return (membership.role, membership.comment, membership.is_active)


@pytest.mark.contract
@pytest.mark.django_db
class TestNonRoleFieldsOnSomeoneElse:
    @pytest.mark.parametrize(
        "workspace_role, project_role",
        [pytest.param(GUEST, GUEST, id="guest"), pytest.param(MEMBER, MEMBER, id="member")],
    )
    @pytest.mark.parametrize(
        "target_workspace_role",
        [MEMBER, ADMIN],
        ids=["target-project-admin", "target-workspace-admin"],
    )
    @pytest.mark.parametrize("data", NON_ROLE_EDITS)
    def test_non_admin_is_refused(
        self, member_of, patch_member, workspace_role, project_role, target_workspace_role, data
    ):
        client, _ = member_of("requester", workspace_role, project_role)
        _, target = member_of("target", target_workspace_role, ADMIN)
        before = snapshot(target)

        response = patch_member(client, target, data)

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        assert response.json() == NOT_MEMBER_EDITOR
        assert snapshot(target) == before

    @pytest.mark.parametrize(
        "workspace_role, project_role",
        [
            pytest.param(MEMBER, ADMIN, id="project-admin"),
            pytest.param(ADMIN, MEMBER, id="workspace-admin-project-member"),
            pytest.param(ADMIN, ADMIN, id="workspace-admin"),
        ],
    )
    @pytest.mark.parametrize(
        "target_workspace_role, target_project_role",
        [
            pytest.param(MEMBER, MEMBER, id="target-member"),
            pytest.param(MEMBER, ADMIN, id="target-project-admin"),
            pytest.param(ADMIN, ADMIN, id="target-workspace-admin"),
        ],
    )
    def test_admin_changes_comment(
        self, member_of, patch_member, workspace_role, project_role, target_workspace_role, target_project_role
    ):
        client, _ = member_of("requester", workspace_role, project_role)
        _, target = member_of("target", target_workspace_role, target_project_role)

        response = patch_member(client, target, {"comment": "x"})

        assert response.status_code == http_status.HTTP_200_OK
        assert response.json()["comment"] == "x"
        target.refresh_from_db()
        assert target.comment == "x"

    @pytest.mark.parametrize(
        "workspace_role, project_role",
        [
            pytest.param(MEMBER, ADMIN, id="project-admin"),
            pytest.param(ADMIN, MEMBER, id="workspace-admin-project-member"),
        ],
    )
    @pytest.mark.parametrize("target_workspace_role", [MEMBER, ADMIN], ids=["target-member", "target-workspace-admin"])
    def test_admin_deactivates(self, member_of, patch_member, workspace_role, project_role, target_workspace_role):
        client, _ = member_of("requester", workspace_role, project_role)
        _, target = member_of("target", target_workspace_role, MEMBER)

        response = patch_member(client, target, {"is_active": False})

        assert response.status_code == http_status.HTTP_200_OK
        assert response.json()["is_active"] is False
        target.refresh_from_db()
        assert target.is_active is False


@pytest.mark.contract
@pytest.mark.django_db
class TestOwnMembership:
    @pytest.mark.parametrize("project_role", [ADMIN, MEMBER], ids=["admin", "member"])
    @pytest.mark.parametrize("is_active", [False, True], ids=["deactivate", "reactivate"])
    def test_workspace_admin_cannot_change_own_status(self, member_of, patch_member, project_role, is_active):
        client, membership = member_of("self-admin", ADMIN, project_role)
        before = snapshot(membership)

        response = patch_member(client, membership, {"is_active": is_active, "comment": "x"})

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.json() == OWN_STATUS
        assert snapshot(membership) == before

    def test_workspace_admin_changes_own_comment(self, member_of, patch_member):
        client, membership = member_of("self-admin", ADMIN, MEMBER)

        response = patch_member(client, membership, {"comment": "x"})

        assert response.status_code == http_status.HTTP_200_OK
        membership.refresh_from_db()
        assert membership.comment == "x"

    @pytest.mark.parametrize("data", NON_ROLE_EDITS)
    def test_non_workspace_admin_keeps_the_own_role_refusal(self, member_of, patch_member, data):
        client, membership = member_of("self", MEMBER, ADMIN)
        before = snapshot(membership)

        response = patch_member(client, membership, data)

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.json() == OWN_ROLE
        assert snapshot(membership) == before
