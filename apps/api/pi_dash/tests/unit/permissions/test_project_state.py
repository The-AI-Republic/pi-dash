# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.contrib.auth.models import AnonymousUser

from pi_dash.app.permissions import ROLE, can_mutate_states
from pi_dash.db.models import (
    Project,
    ProjectMember,
    User,
    Workspace,
    WorkspaceMember,
)


@pytest.fixture
def other_user(db):
    # Distinct username so this fixture composes with create_user (which
    # leaves username at the model default of empty string). Without it,
    # tests that pull both fixtures collide on users_username_key.
    return User.objects.create(
        username="other",
        email="other@example.com",
        first_name="Other",
        last_name="User",
    )


@pytest.fixture
def project(workspace):
    """Project with members_can_edit_states defaulted to True."""
    return Project.objects.create(
        name="Test Project",
        identifier="TST",
        workspace=workspace,
    )


def _set_flag(project, value):
    Project.objects.filter(pk=project.pk).update(members_can_edit_states=value)


def _add_project_member(workspace, project, user, role):
    return ProjectMember.objects.create(
        workspace=workspace,
        project=project,
        member=user,
        role=role,
        is_active=True,
    )


def _add_workspace_admin(workspace, user):
    return WorkspaceMember.objects.create(workspace=workspace, member=user, role=ROLE.ADMIN.value)


@pytest.mark.unit
class TestCanMutateStates:
    """Permission-matrix tests for `can_mutate_states`."""

    @pytest.mark.django_db
    def test_anonymous_denied(self, workspace, project):
        assert can_mutate_states(AnonymousUser(), workspace.slug, project.pk) is False

    @pytest.mark.django_db
    def test_non_member_denied(self, workspace, project, other_user):
        assert can_mutate_states(other_user, workspace.slug, project.pk) is False

    @pytest.mark.django_db
    def test_project_admin_allowed_flag_on(self, workspace, project, other_user):
        _add_project_member(workspace, project, other_user, ROLE.ADMIN.value)
        assert can_mutate_states(other_user, workspace.slug, project.pk) is True

    @pytest.mark.django_db
    def test_project_admin_allowed_flag_off(self, workspace, project, other_user):
        _add_project_member(workspace, project, other_user, ROLE.ADMIN.value)
        _set_flag(project, False)
        assert can_mutate_states(other_user, workspace.slug, project.pk) is True

    @pytest.mark.django_db
    def test_project_member_allowed_when_flag_on(self, workspace, project, other_user):
        _add_project_member(workspace, project, other_user, ROLE.MEMBER.value)
        assert can_mutate_states(other_user, workspace.slug, project.pk) is True

    @pytest.mark.django_db
    def test_project_member_denied_when_flag_off(self, workspace, project, other_user):
        _add_project_member(workspace, project, other_user, ROLE.MEMBER.value)
        _set_flag(project, False)
        assert can_mutate_states(other_user, workspace.slug, project.pk) is False

    @pytest.mark.django_db
    def test_workspace_admin_override_for_project_member_flag_off(self, workspace, project, other_user):
        _add_project_member(workspace, project, other_user, ROLE.MEMBER.value)
        _add_workspace_admin(workspace, other_user)
        _set_flag(project, False)
        assert can_mutate_states(other_user, workspace.slug, project.pk) is True

    @pytest.mark.django_db
    def test_project_guest_denied_flag_on(self, workspace, project, other_user):
        _add_project_member(workspace, project, other_user, ROLE.GUEST.value)
        assert can_mutate_states(other_user, workspace.slug, project.pk) is False

    @pytest.mark.django_db
    def test_project_guest_denied_flag_off(self, workspace, project, other_user):
        _add_project_member(workspace, project, other_user, ROLE.GUEST.value)
        _set_flag(project, False)
        assert can_mutate_states(other_user, workspace.slug, project.pk) is False

    @pytest.mark.django_db
    def test_workspace_admin_override_for_project_guest(self, workspace, project, other_user):
        _add_project_member(workspace, project, other_user, ROLE.GUEST.value)
        _add_workspace_admin(workspace, other_user)
        _set_flag(project, False)
        assert can_mutate_states(other_user, workspace.slug, project.pk) is True

    @pytest.mark.django_db
    def test_inactive_project_member_denied(self, workspace, project, other_user):
        ProjectMember.objects.create(
            workspace=workspace,
            project=project,
            member=other_user,
            role=ROLE.MEMBER.value,
            is_active=False,
        )
        assert can_mutate_states(other_user, workspace.slug, project.pk) is False

    @pytest.mark.django_db
    def test_workspace_admin_without_project_membership_denied(self, workspace, project, other_user):
        _add_workspace_admin(workspace, other_user)
        assert can_mutate_states(other_user, workspace.slug, project.pk) is False

    @pytest.mark.django_db
    def test_inactive_workspace_admin_does_not_override(self, workspace, project, other_user):
        _add_project_member(workspace, project, other_user, ROLE.MEMBER.value)
        WorkspaceMember.objects.create(
            workspace=workspace,
            member=other_user,
            role=ROLE.ADMIN.value,
            is_active=False,
        )
        _set_flag(project, False)
        assert can_mutate_states(other_user, workspace.slug, project.pk) is False
