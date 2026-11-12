# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for project-guest reads on modules (NEWFRONT-334).

Guests may read a module's detail and its work items (read-only). The
work-item list applies the issue-list guest rule: a guest only sees the
issues they created unless ``project.guest_view_all_features`` is set.
Every write stays admin/member-only and non-members stay refused.
"""

from unittest.mock import patch
from uuid import uuid4

import pytest
from rest_framework import status
from rest_framework.test import APIClient

from pi_dash.db.models import (
    Issue,
    Module,
    ModuleIssue,
    ProjectMember,
    State,
    User,
    WorkspaceMember,
)


def _module_url(slug, project_id, module_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/"


def _module_issues_url(slug, project_id, module_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/"


def _modules_url(slug, project_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/modules/"


@pytest.fixture
def member_user(db):
    return User.objects.create_user(email="member@example.com", username="member")


@pytest.fixture
def guest_user(db):
    return User.objects.create_user(email="guest@example.com", username="guest")


@pytest.fixture
def outsider_user(db):
    return User.objects.create_user(email="outsider@example.com", username="outsider")


@pytest.fixture
def memberships(workspace, project, create_user, member_user, guest_user):
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=member_user, role=15)
    ProjectMember.objects.create(project=project, member=member_user, role=15, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=guest_user, role=5)
    ProjectMember.objects.create(project=project, member=guest_user, role=5, is_active=True)


@pytest.fixture
def module(workspace, project, create_user):
    return Module.objects.create(
        name="Guest readable module",
        project=project,
        workspace=workspace,
        created_by=create_user,
    )


@pytest.fixture
def module_state(workspace, project, create_user):
    return State.objects.create(
        name="Todo",
        project=project,
        workspace=workspace,
        group="unstarted",
        default=True,
        created_by=create_user,
    )


def _make_issue(name, project, workspace, state, creator):
    # BaseModel.save() overwrites created_by from the request user (None in
    # tests), so save explicitly to keep the creator the scoping relies on.
    issue = Issue(
        name=name,
        description_html="<p>body</p>",
        description_stripped="body",
        project=project,
        workspace=workspace,
        state=state,
        created_by=creator,
    )
    issue.save(disable_auto_set_user=True)
    return issue


@pytest.fixture
def admin_issue(workspace, project, create_user, module_state):
    return _make_issue("admin issue", project, workspace, module_state, create_user)


@pytest.fixture
def guest_issue(workspace, project, guest_user, module_state):
    return _make_issue("guest issue", project, workspace, module_state, guest_user)


@pytest.fixture
def module_issues(workspace, project, create_user, module, admin_issue, guest_issue):
    ModuleIssue.objects.create(
        module=module,
        issue=admin_issue,
        project=project,
        workspace=workspace,
        created_by=create_user,
    )
    ModuleIssue.objects.create(
        module=module,
        issue=guest_issue,
        project=project,
        workspace=workspace,
        created_by=create_user,
    )


@pytest.fixture(autouse=True)
def no_celery():
    with patch("pi_dash.app.views.module.base.recent_visited_task"):
        yield


def _client_for(user):
    client = APIClient()
    client.force_authenticate(user=user)
    return client


def _result_ids(response):
    return {str(row["id"]) for row in response.data["results"]}


@pytest.mark.contract
class TestModuleGuestReads:
    @pytest.mark.django_db
    def test_guest_module_detail_returns_200(self, workspace, project, guest_user, memberships, module):
        response = _client_for(guest_user).get(_module_url(workspace.slug, project.id, module.id))

        assert response.status_code == status.HTTP_200_OK
        assert response.data["name"] == module.name

    @pytest.mark.django_db
    def test_guest_module_list_still_returns_200(self, workspace, project, guest_user, memberships, module):
        response = _client_for(guest_user).get(_modules_url(workspace.slug, project.id))

        assert response.status_code == status.HTTP_200_OK

    @pytest.mark.django_db
    def test_guest_module_issues_scoped_to_own_issues_by_default(
        self, workspace, project, guest_user, memberships, module, module_issues, guest_issue
    ):
        response = _client_for(guest_user).get(_module_issues_url(workspace.slug, project.id, module.id))

        assert response.status_code == status.HTTP_200_OK
        assert response.data["count"] == 1
        assert _result_ids(response) == {str(guest_issue.id)}

    @pytest.mark.django_db
    def test_guest_module_issues_sees_all_with_view_all_flag(
        self,
        workspace,
        project,
        guest_user,
        memberships,
        module,
        module_issues,
        admin_issue,
        guest_issue,
    ):
        project.guest_view_all_features = True
        project.save()

        response = _client_for(guest_user).get(_module_issues_url(workspace.slug, project.id, module.id))

        assert response.status_code == status.HTTP_200_OK
        assert response.data["count"] == 2
        assert _result_ids(response) == {str(admin_issue.id), str(guest_issue.id)}

    @pytest.mark.django_db
    def test_member_reads_still_return_200(self, workspace, project, member_user, memberships, module, module_issues):
        client = _client_for(member_user)

        detail = client.get(_module_url(workspace.slug, project.id, module.id))
        issues = client.get(_module_issues_url(workspace.slug, project.id, module.id))

        assert detail.status_code == status.HTTP_200_OK
        assert issues.status_code == status.HTTP_200_OK
        assert issues.data["count"] == 2

    @pytest.mark.django_db
    def test_unknown_module_still_returns_404(self, workspace, project, member_user, memberships):
        response = _client_for(member_user).get(_module_url(workspace.slug, project.id, uuid4()))

        assert response.status_code == status.HTTP_404_NOT_FOUND


@pytest.mark.contract
class TestModuleGuestWritesRefused:
    @pytest.mark.django_db
    def test_guest_create_module_refused(self, workspace, project, guest_user, memberships):
        response = _client_for(guest_user).post(
            _modules_url(workspace.slug, project.id), {"name": "nope"}, format="json"
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_guest_partial_update_module_refused(self, workspace, project, guest_user, memberships, module):
        response = _client_for(guest_user).patch(
            _module_url(workspace.slug, project.id, module.id), {"name": "nope"}, format="json"
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_guest_destroy_module_refused(self, workspace, project, guest_user, memberships, module):
        response = _client_for(guest_user).delete(_module_url(workspace.slug, project.id, module.id))

        assert response.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_guest_create_module_issues_refused(self, workspace, project, guest_user, memberships, module, admin_issue):
        response = _client_for(guest_user).post(
            _module_issues_url(workspace.slug, project.id, module.id),
            {"issues": [str(admin_issue.id)]},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_guest_destroy_module_issue_refused(
        self, workspace, project, guest_user, memberships, module, module_issues, guest_issue
    ):
        response = _client_for(guest_user).delete(
            f"{_module_issues_url(workspace.slug, project.id, module.id)}{guest_issue.id}/"
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_member_partial_update_still_returns_200(self, workspace, project, member_user, memberships, module):
        with patch("pi_dash.app.views.module.base.model_activity"):
            response = _client_for(member_user).patch(
                _module_url(workspace.slug, project.id, module.id),
                {"name": "renamed by member"},
                format="json",
            )

        assert response.status_code == status.HTTP_200_OK
        assert response.data["name"] == "renamed by member"


@pytest.mark.contract
class TestModuleNonMemberRefused:
    @pytest.mark.django_db
    def test_outsider_detail_refused(self, workspace, project, outsider_user, module):
        response = _client_for(outsider_user).get(_module_url(workspace.slug, project.id, module.id))

        assert response.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_outsider_issues_refused(self, workspace, project, outsider_user, module, module_issues):
        response = _client_for(outsider_user).get(_module_issues_url(workspace.slug, project.id, module.id))

        assert response.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_outsider_list_refused(self, workspace, project, outsider_user, module):
        response = _client_for(outsider_user).get(_modules_url(workspace.slug, project.id))

        assert response.status_code == status.HTTP_403_FORBIDDEN
