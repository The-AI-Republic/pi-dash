# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for project view retrieve (VIEW-022, NEWFRONT-227) and the
view access/publish write path (VIEW-019, VIEW-020, NEWFRONT-972).

An unknown project view id must miss with 404. A view that exists but is
private to another user and hidden from guests must keep failing closed with
403, rather than being conflated with a missing row.

The owner may store `access` on either scope and the publish `anchor` on the
project scope; the PATCH echo carries the stored value and a follow-up GET
reports it persisted. Non-owners cannot write: project scope refuses with the
owner error, workspace scope with the creator-gated 403.
"""

from unittest.mock import patch
from uuid import uuid4

import pytest
from rest_framework import status

from pi_dash.db.models import IssueView, ProjectMember, User, WorkspaceMember


def _view_url(slug, project_id, view_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/views/{view_id}/"


def _workspace_view_url(slug, view_id):
    return f"/api/workspaces/{slug}/views/{view_id}/"


def _make_view(name, workspace, owner, project=None, description="Things in flight"):
    # BaseModel.save() overwrites created_by from the request user (None in
    # tests), so save explicitly to keep the creator the creator-gated
    # partial_update decorator relies on (same as the worklog suites).
    view = IssueView(
        name=name,
        description=description,
        filters={},
        project=project,
        workspace=workspace,
        owned_by=owner,
        created_by=owner,
    )
    view.save(disable_auto_set_user=True)
    return view


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
    return _make_view("Active issues", workspace, create_user, project=project)


@pytest.fixture
def workspace_view(workspace, create_user):
    return _make_view("Workspace triage", workspace, create_user)


@pytest.fixture
def member_user(workspace, project):
    # A project member and workspace admin who owns no views: on the project
    # scope the decorator lets members through and the owner check refuses
    # them; on the workspace scope the creator gate refuses them first.
    user = User.objects.create_user(email="view-member@example.com", username="view-member")
    WorkspaceMember.objects.create(workspace=workspace, member=user, role=20)
    ProjectMember.objects.create(project=project, member=user, role=15, is_active=True)
    return user


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


@pytest.mark.contract
@pytest.mark.django_db
class TestProjectViewAccessWrite:
    @patch("pi_dash.app.views.view.base.recent_visited_task")
    def test_owner_patch_access_echoes_and_persists(
        self, recent_visited, session_client, workspace, project, project_view
    ):
        assert project_view.access == 1

        url = _view_url(workspace.slug, project.id, project_view.id)
        response = session_client.patch(url, {"access": 0}, format="json")

        assert response.status_code == status.HTTP_200_OK
        assert response.data["access"] == 0
        assert session_client.get(url).data["access"] == 0
        recent_visited.delay.assert_called_once()

    def test_create_without_access_keeps_default(self, session_client, workspace, project, member_membership):
        response = session_client.post(
            f"/api/workspaces/{workspace.slug}/projects/{project.id}/views/",
            {"name": "Fresh view"},
            format="json",
        )

        assert response.status_code == status.HTTP_201_CREATED
        assert response.data["access"] == 1
        assert response.data["anchor"] is None

    def test_non_owner_patch_access_is_refused(self, api_client, workspace, project, project_view, member_user):
        api_client.force_authenticate(user=member_user)

        response = api_client.patch(
            _view_url(workspace.slug, project.id, project_view.id), {"access": 0}, format="json"
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "Only the owner of the view can update the view"}
        project_view.refresh_from_db()
        assert project_view.access == 1

    def test_invalid_access_is_rejected(self, session_client, workspace, project, project_view):
        response = session_client.patch(
            _view_url(workspace.slug, project.id, project_view.id), {"access": 7}, format="json"
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert "access" in response.data
        project_view.refresh_from_db()
        assert project_view.access == 1


@pytest.mark.contract
@pytest.mark.django_db
class TestProjectViewAnchorWrite:
    @patch("pi_dash.app.views.view.base.recent_visited_task")
    def test_owner_publish_echoes_and_persists(self, recent_visited, session_client, workspace, project, project_view):
        assert project_view.anchor is None

        url = _view_url(workspace.slug, project.id, project_view.id)
        response = session_client.patch(url, {"anchor": "view-anchor-1"}, format="json")

        assert response.status_code == status.HTTP_200_OK
        assert response.data["anchor"] == "view-anchor-1"
        assert session_client.get(url).data["anchor"] == "view-anchor-1"
        recent_visited.delay.assert_called_once()

    @patch("pi_dash.app.views.view.base.recent_visited_task")
    def test_owner_unpublish_clears(
        self, recent_visited, session_client, workspace, project, create_user, project_view
    ):
        # A second unpublished view shares anchor=None, proving the clear is
        # not rejected as a uniqueness conflict.
        _make_view("Other view", workspace, create_user, project=project)
        session_client.patch(
            _view_url(workspace.slug, project.id, project_view.id),
            {"anchor": "view-anchor-1"},
            format="json",
        )

        url = _view_url(workspace.slug, project.id, project_view.id)
        response = session_client.patch(url, {"anchor": None}, format="json")

        assert response.status_code == status.HTTP_200_OK
        assert response.data["anchor"] is None
        assert session_client.get(url).data["anchor"] is None

    def test_duplicate_anchor_is_rejected(self, session_client, workspace, project, create_user, project_view):
        session_client.patch(
            _view_url(workspace.slug, project.id, project_view.id),
            {"anchor": "view-anchor-1"},
            format="json",
        )
        other = _make_view("Other view", workspace, create_user, project=project)

        response = session_client.patch(
            _view_url(workspace.slug, project.id, other.id), {"anchor": "view-anchor-1"}, format="json"
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert "anchor" in response.data
        other.refresh_from_db()
        assert other.anchor is None

    def test_republish_same_anchor_is_idempotent(self, session_client, workspace, project, project_view):
        url = _view_url(workspace.slug, project.id, project_view.id)
        first = session_client.patch(url, {"anchor": "view-anchor-1"}, format="json")
        second = session_client.patch(url, {"anchor": "view-anchor-1"}, format="json")

        assert first.status_code == status.HTTP_200_OK
        assert second.status_code == status.HTTP_200_OK
        assert second.data["anchor"] == "view-anchor-1"

    def test_non_owner_publish_is_refused(self, api_client, workspace, project, project_view, member_user):
        api_client.force_authenticate(user=member_user)

        response = api_client.patch(
            _view_url(workspace.slug, project.id, project_view.id),
            {"anchor": "view-anchor-1"},
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "Only the owner of the view can update the view"}
        project_view.refresh_from_db()
        assert project_view.anchor is None


@pytest.mark.contract
@pytest.mark.django_db
class TestWorkspaceViewAccessWrite:
    @patch("pi_dash.app.views.view.base.recent_visited_task")
    def test_owner_patch_access_echoes_and_persists(self, recent_visited, session_client, workspace, workspace_view):
        assert workspace_view.access == 1

        url = _workspace_view_url(workspace.slug, workspace_view.id)
        response = session_client.patch(url, {"access": 0}, format="json")

        assert response.status_code == status.HTTP_200_OK
        assert response.data["access"] == 0
        assert session_client.get(url).data["access"] == 0
        recent_visited.delay.assert_called_once()

    def test_non_owner_patch_access_is_refused(self, api_client, workspace, project, workspace_view, member_user):
        api_client.force_authenticate(user=member_user)

        response = api_client.patch(
            _workspace_view_url(workspace.slug, workspace_view.id), {"access": 0}, format="json"
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN
        assert response.data == {"error": "You don't have the required permissions."}
        workspace_view.refresh_from_db()
        assert workspace_view.access == 1
