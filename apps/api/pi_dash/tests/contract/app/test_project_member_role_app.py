# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Who may change a project membership (``ProjectMemberViewSet.partial_update``).

Authority comes from the requester's workspace role. The target's workspace
role only ever denies: nobody else can change a workspace admin's role, and a
workspace guest cannot hold project role Member or Admin.

Any field on someone else's membership, not just ``role``, needs a project
Admin or a workspace admin (PDASHOSS01-329), and nobody can change their own
``is_active`` here.
"""

import pytest
from rest_framework import status as http_status
from rest_framework.test import APIClient

from pi_dash.db.models import ProjectMember, User, WorkspaceMember

ADMIN, MEMBER, GUEST = 20, 15, 5

OWN_ROLE = {"error": "You cannot update your own role"}
NOT_ADMIN = {"error": "You do not have permission to update roles"}
TARGET_TOO_HIGH = {"error": "You cannot update the role of a member with a role equal to or higher than your own"}
ASSIGN_TOO_HIGH = {"error": "You cannot assign a role equal to or higher than your own"}
GUEST_CAP = {"error": "You cannot add a user with role higher than the workspace role"}
WORKSPACE_ADMIN_TARGET = {"error": "You cannot update the role of a workspace admin"}
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
def set_role(project):
    def _set_role(client, membership, role):
        return client.patch(
            f"/api/workspaces/{project.workspace.slug}/projects/{project.id}/members/{membership.id}/",
            {"role": role},
            format="json",
        )

    return _set_role


@pytest.fixture
def patch_member(project):
    def _patch(client, membership, data):
        return client.patch(
            f"/api/workspaces/{project.workspace.slug}/projects/{project.id}/members/{membership.id}/",
            data,
            format="json",
        )

    return _patch


def assert_refused(response, expected_status, expected_body, membership):
    role_before = membership.role
    assert response.status_code == expected_status
    assert response.json() == expected_body
    membership.refresh_from_db()
    assert membership.role == role_before


def assert_changed(response, membership, role):
    assert response.status_code == http_status.HTTP_200_OK
    assert response.json()["role"] == role
    membership.refresh_from_db()
    assert membership.role == role


@pytest.mark.contract
@pytest.mark.django_db
class TestWorkspaceAdminTarget:
    @pytest.mark.parametrize(
        "workspace_role, project_role",
        [
            pytest.param(MEMBER, MEMBER, id="plain-member"),
            pytest.param(GUEST, GUEST, id="guest"),
            pytest.param(MEMBER, ADMIN, id="project-admin"),
            pytest.param(ADMIN, ADMIN, id="other-workspace-admin"),
            pytest.param(ADMIN, MEMBER, id="other-workspace-admin-project-member"),
        ],
    )
    @pytest.mark.parametrize("target_project_role", [ADMIN, MEMBER], ids=["target-admin", "target-member"])
    def test_nobody_else_changes_a_workspace_admins_role(
        self, member_of, set_role, workspace_role, project_role, target_project_role
    ):
        client, _ = member_of("requester", workspace_role, project_role)
        _, target = member_of("target", ADMIN, target_project_role)

        response = set_role(client, target, GUEST)

        assert_refused(response, http_status.HTTP_403_FORBIDDEN, WORKSPACE_ADMIN_TARGET, target)

    @pytest.mark.parametrize("project_role", [ADMIN, MEMBER, GUEST], ids=["admin", "member", "guest"])
    def test_workspace_admin_changes_their_own_role(self, member_of, set_role, project_role):
        client, membership = member_of("self-admin", ADMIN, project_role)
        new_role = MEMBER if project_role != MEMBER else ADMIN

        assert_changed(set_role(client, membership, new_role), membership, new_role)


@pytest.mark.contract
@pytest.mark.django_db
class TestWorkspaceAdminRequester:
    @pytest.mark.parametrize("project_role", [ADMIN, MEMBER, GUEST], ids=["admin", "member", "guest"])
    def test_promotes_a_member_to_project_admin(self, member_of, set_role, project_role):
        client, _ = member_of("ws-admin", ADMIN, project_role)
        _, target = member_of("target", MEMBER, MEMBER)

        assert_changed(set_role(client, target, ADMIN), target, ADMIN)

    def test_project_member_demotes_a_project_admin_who_is_not_a_workspace_admin(self, member_of, set_role):
        client, _ = member_of("ws-admin", ADMIN, MEMBER)
        _, target = member_of("target", MEMBER, ADMIN)

        assert_changed(set_role(client, target, GUEST), target, GUEST)


@pytest.mark.contract
@pytest.mark.django_db
class TestEveryoneElseKeepsTheGuards:
    def test_project_admin_cannot_promote_to_admin(self, member_of, set_role):
        client, _ = member_of("proj-admin", MEMBER, ADMIN)
        _, target = member_of("target", MEMBER, MEMBER)

        assert_refused(set_role(client, target, ADMIN), http_status.HTTP_403_FORBIDDEN, ASSIGN_TOO_HIGH, target)

    def test_project_admin_cannot_change_another_project_admin(self, member_of, set_role):
        client, _ = member_of("proj-admin", MEMBER, ADMIN)
        _, target = member_of("target", MEMBER, ADMIN)

        assert_refused(set_role(client, target, MEMBER), http_status.HTTP_403_FORBIDDEN, TARGET_TOO_HIGH, target)

    def test_project_admin_changes_a_lower_member(self, member_of, set_role):
        client, _ = member_of("proj-admin", MEMBER, ADMIN)
        _, target = member_of("target", MEMBER, MEMBER)

        assert_changed(set_role(client, target, GUEST), target, GUEST)

    @pytest.mark.parametrize(
        "workspace_role, project_role",
        [pytest.param(MEMBER, MEMBER, id="member"), pytest.param(GUEST, GUEST, id="guest")],
    )
    def test_non_admin_cannot_change_roles(self, member_of, set_role, workspace_role, project_role):
        client, _ = member_of("requester", workspace_role, project_role)
        _, target = member_of("target", MEMBER, MEMBER)

        assert_refused(set_role(client, target, GUEST), http_status.HTTP_403_FORBIDDEN, NOT_ADMIN, target)

    @pytest.mark.parametrize(
        "workspace_role, project_role",
        [pytest.param(MEMBER, ADMIN, id="project-admin"), pytest.param(MEMBER, MEMBER, id="member")],
    )
    def test_non_workspace_admin_cannot_change_their_own_role(self, member_of, set_role, workspace_role, project_role):
        client, membership = member_of("self", workspace_role, project_role)

        assert_refused(set_role(client, membership, GUEST), http_status.HTTP_400_BAD_REQUEST, OWN_ROLE, membership)


@pytest.mark.contract
@pytest.mark.django_db
class TestWorkspaceGuestCap:
    @pytest.mark.parametrize("new_role", [ADMIN, MEMBER], ids=["to-admin", "to-member"])
    def test_workspace_admin_is_capped(self, member_of, set_role, new_role):
        client, _ = member_of("ws-admin", ADMIN, MEMBER)
        _, target = member_of("target", GUEST, GUEST)

        assert_refused(set_role(client, target, new_role), http_status.HTTP_400_BAD_REQUEST, GUEST_CAP, target)

    def test_project_admin_is_capped(self, member_of, set_role):
        client, _ = member_of("proj-admin", MEMBER, ADMIN)
        _, target = member_of("target", GUEST, GUEST)

        assert_refused(set_role(client, target, MEMBER), http_status.HTTP_400_BAD_REQUEST, GUEST_CAP, target)


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
