# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for project work-item types (NEWFRONT-984, ISS-129/234/235).

Types are workspace rows linked onto projects; the cloud type picker,
type filter and detail badge all read through the project list. Reads
are open to every active project member; creation is admin-only, like
labels.
"""

import pytest
from rest_framework import status
from rest_framework.test import APIClient

from pi_dash.db.models import (
    IssueType,
    ProjectMember,
    User,
    WorkspaceMember,
)
from pi_dash.db.models.issue_type import ProjectIssueType


def _collection_url(slug, project_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/issue-types/"


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


def _make_type(name, workspace, project, creator, **overrides):
    issue_type = IssueType.objects.create(
        name=name,
        workspace=workspace,
        created_by=creator,
        **overrides,
    )
    ProjectIssueType.objects.create(
        project=project,
        workspace=workspace,
        issue_type=issue_type,
        level=0,
    )
    return issue_type


def _client_for(user):
    client = APIClient()
    client.force_authenticate(user=user)
    return client


@pytest.mark.contract
class TestIssueTypeList:
    @pytest.mark.django_db
    def test_member_lists_linked_types(self, workspace, project, create_user, member_user, memberships):
        first = _make_type("Bug", workspace, project, create_user)
        second = _make_type("Epic", workspace, project, create_user, is_epic=True)
        # A workspace type the project never linked stays out of the list.
        IssueType.objects.create(name="Task", workspace=workspace, created_by=create_user)

        response = _client_for(member_user).get(_collection_url(workspace.slug, project.id))

        assert response.status_code == status.HTTP_200_OK
        assert [row["name"] for row in response.data] == ["Bug", "Epic"]
        by_id = {str(row["id"]): row for row in response.data}
        assert by_id[str(first.id)]["is_epic"] is False
        assert by_id[str(second.id)]["is_epic"] is True

    @pytest.mark.django_db
    def test_guest_lists_types(self, workspace, project, create_user, guest_user, memberships):
        _make_type("Bug", workspace, project, create_user)

        response = _client_for(guest_user).get(_collection_url(workspace.slug, project.id))

        assert response.status_code == status.HTTP_200_OK
        assert [row["name"] for row in response.data] == ["Bug"]

    @pytest.mark.django_db
    def test_outsider_is_refused(self, workspace, project, create_user, outsider_user, memberships):
        _make_type("Bug", workspace, project, create_user)

        response = _client_for(outsider_user).get(_collection_url(workspace.slug, project.id))

        assert response.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_empty_project_lists_nothing(self, workspace, project, member_user, memberships):
        response = _client_for(member_user).get(_collection_url(workspace.slug, project.id))

        assert response.status_code == status.HTTP_200_OK
        assert response.data == []


@pytest.mark.contract
class TestIssueTypeCreate:
    @pytest.mark.django_db
    def test_admin_creates_and_links_type(self, workspace, project, create_user, memberships):
        response = _client_for(create_user).post(
            _collection_url(workspace.slug, project.id),
            {"name": "Spike", "description": "Time-boxed research", "is_epic": False},
            format="json",
        )

        assert response.status_code == status.HTTP_201_CREATED
        assert response.data["name"] == "Spike"
        assert response.data["is_epic"] is False
        stored = IssueType.objects.get(pk=response.data["id"])
        assert stored.workspace_id == workspace.id
        assert ProjectIssueType.objects.filter(
            project=project, issue_type=stored, deleted_at__isnull=True
        ).exists()

    @pytest.mark.django_db
    def test_admin_creates_epic_type(self, workspace, project, create_user, memberships):
        response = _client_for(create_user).post(
            _collection_url(workspace.slug, project.id),
            {"name": "Epic", "is_epic": True},
            format="json",
        )

        assert response.status_code == status.HTTP_201_CREATED
        assert response.data["is_epic"] is True

    @pytest.mark.django_db
    def test_blank_name_is_refused(self, workspace, project, create_user, memberships):
        response = _client_for(create_user).post(
            _collection_url(workspace.slug, project.id),
            {"name": "   "},
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST

    @pytest.mark.django_db
    def test_member_cannot_create(self, workspace, project, member_user, memberships):
        response = _client_for(member_user).post(
            _collection_url(workspace.slug, project.id),
            {"name": "Spike"},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN
