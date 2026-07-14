# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the workspace join-request flow (request-to-join by
admin email).

Covers the requester side (create a request under ``/users/me/`` and list
one's own requests) and the admin side (list pending requests for a workspace,
approve, deny).
"""

import pytest
from django.urls import reverse
from rest_framework import status
from rest_framework.test import APIClient

from pi_dash.db.models import (
    Profile,
    User,
    Workspace,
    WorkspaceJoinRequest,
    WorkspaceMember,
)


ADMIN_ROLE = 20
MEMBER_ROLE = 15


@pytest.fixture
def admin_user(db):
    """A user who owns and administers a workspace."""
    user = User.objects.create(email="admin@example.com", username="admin_user", first_name="Admin", last_name="User")
    user.set_password("admin-password")
    user.save()
    return user


@pytest.fixture
def admin_workspace(admin_user):
    """A workspace owned by ``admin_user`` with an active Admin membership."""
    ws = Workspace.objects.create(name="Acme", owner=admin_user, slug="acme")
    WorkspaceMember.objects.create(workspace=ws, member=admin_user, role=ADMIN_ROLE)
    return ws


@pytest.fixture
def admin_client(admin_user):
    """API client authenticated as the workspace admin."""
    client = APIClient()
    client.force_authenticate(user=admin_user)
    return client


@pytest.mark.contract
class TestCreateJoinRequest:
    """POST /api/users/me/workspaces/join-requests/ (requester side)."""

    @pytest.mark.django_db
    def test_request_resolves_to_admin_workspace(self, session_client, admin_workspace):
        """Typing a real admin email creates a PENDING request against that workspace."""
        url = reverse("user-workspace-join-requests")
        response = session_client.post(url, {"admin_email": "admin@example.com"}, format="json")

        assert response.status_code == status.HTTP_201_CREATED
        assert response.data["message"] == "Request sent"

        jr = WorkspaceJoinRequest.objects.get()
        assert jr.workspace_id == admin_workspace.id
        assert jr.admin_email == "admin@example.com"
        assert jr.status == WorkspaceJoinRequest.Status.PENDING

    @pytest.mark.django_db
    def test_unknown_email_still_returns_neutral_response(self, session_client):
        """An email that is nobody's admin returns the SAME response (no enumeration
        leak) and records an unresolved (workspace=None) request."""
        url = reverse("user-workspace-join-requests")
        response = session_client.post(url, {"admin_email": "nobody@example.com"}, format="json")

        assert response.status_code == status.HTTP_201_CREATED
        assert response.data["message"] == "Request sent"

        jr = WorkspaceJoinRequest.objects.get()
        assert jr.workspace_id is None
        assert jr.admin_email == "nobody@example.com"
        assert jr.status == WorkspaceJoinRequest.Status.PENDING

    @pytest.mark.django_db
    def test_cannot_request_with_own_email(self, session_client, create_user):
        url = reverse("user-workspace-join-requests")
        response = session_client.post(url, {"admin_email": create_user.email}, format="json")

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert WorkspaceJoinRequest.objects.count() == 0

    @pytest.mark.django_db
    def test_invalid_email_rejected(self, session_client):
        url = reverse("user-workspace-join-requests")
        response = session_client.post(url, {"admin_email": "not-an-email"}, format="json")

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert WorkspaceJoinRequest.objects.count() == 0

    @pytest.mark.django_db
    def test_duplicate_request_is_idempotent(self, session_client, admin_workspace):
        url = reverse("user-workspace-join-requests")
        session_client.post(url, {"admin_email": "admin@example.com"}, format="json")
        session_client.post(url, {"admin_email": "admin@example.com"}, format="json")

        assert (
            WorkspaceJoinRequest.objects.filter(
                workspace=admin_workspace, status=WorkspaceJoinRequest.Status.PENDING
            ).count()
            == 1
        )

    @pytest.mark.django_db
    def test_requires_authentication(self, api_client, admin_workspace):
        url = reverse("user-workspace-join-requests")
        response = api_client.post(url, {"admin_email": "admin@example.com"}, format="json")
        assert response.status_code in (status.HTTP_401_UNAUTHORIZED, status.HTTP_403_FORBIDDEN)


@pytest.mark.contract
class TestListOwnJoinRequests:
    """GET /api/users/me/workspaces/join-requests/ (requester side)."""

    @pytest.mark.django_db
    def test_lists_only_own_requests(self, session_client, create_user, admin_user, admin_workspace):
        # A request by the current user
        WorkspaceJoinRequest.objects.create(
            requester=create_user, workspace=admin_workspace, admin_email="admin@example.com"
        )
        # A request by someone else — must not appear
        other = User.objects.create(email="other@example.com", username="other_user")
        WorkspaceJoinRequest.objects.create(requester=other, workspace=admin_workspace, admin_email="admin@example.com")

        url = reverse("user-workspace-join-requests")
        response = session_client.get(url)

        assert response.status_code == status.HTTP_200_OK
        results = response.data["results"] if isinstance(response.data, dict) else response.data
        assert len(results) == 1
        assert str(results[0]["requester"]["id"]) == str(create_user.id)

    @pytest.mark.django_db
    def test_own_request_list_does_not_leak_workspace(self, session_client, create_user, admin_workspace):
        """The requester's own list must not expose the resolved workspace.

        Exposing it would let a requester enumerate real workspace admins by
        submitting emails and reading back their own request list — defeating the
        neutral create response.
        """
        WorkspaceJoinRequest.objects.create(
            requester=create_user, workspace=admin_workspace, admin_email="admin@example.com"
        )

        url = reverse("user-workspace-join-requests")
        response = session_client.get(url)

        assert response.status_code == status.HTTP_200_OK
        results = response.data["results"] if isinstance(response.data, dict) else response.data
        assert len(results) == 1
        # No workspace identity (name/slug/id/logo) may appear in the payload.
        assert "workspace" not in results[0]
        assert admin_workspace.slug not in str(results[0])
        assert admin_workspace.name not in str(results[0])


@pytest.mark.contract
class TestAdminListJoinRequests:
    """GET /api/workspaces/<slug>/join-requests/ (admin side)."""

    @pytest.mark.django_db
    def test_admin_lists_pending_requests(self, admin_client, create_user, admin_workspace):
        WorkspaceJoinRequest.objects.create(
            requester=create_user, workspace=admin_workspace, admin_email="admin@example.com"
        )
        url = reverse("workspace-join-requests", kwargs={"slug": admin_workspace.slug})
        response = admin_client.get(url)

        assert response.status_code == status.HTTP_200_OK
        results = response.data["results"] if isinstance(response.data, dict) else response.data
        assert len(results) == 1
        assert str(results[0]["requester"]["id"]) == str(create_user.id)

    @pytest.mark.django_db
    def test_non_member_cannot_list(self, session_client, create_user, admin_workspace):
        """``create_user`` (session_client) is not a member of Acme."""
        WorkspaceJoinRequest.objects.create(
            requester=create_user, workspace=admin_workspace, admin_email="admin@example.com"
        )
        url = reverse("workspace-join-requests", kwargs={"slug": admin_workspace.slug})
        response = session_client.get(url)
        assert response.status_code == status.HTTP_403_FORBIDDEN


@pytest.mark.contract
class TestApproveDenyJoinRequest:
    """POST approve / deny (admin side)."""

    @pytest.mark.django_db
    def test_approve_creates_membership(self, mocker, admin_client, create_user, admin_workspace):
        mocker.patch("pi_dash.app.views.workspace.join_request.track_event.delay")
        profile = Profile.objects.create(user=create_user)
        jr = WorkspaceJoinRequest.objects.create(
            requester=create_user, workspace=admin_workspace, admin_email="admin@example.com"
        )
        url = reverse("workspace-join-request-approve", kwargs={"slug": admin_workspace.slug, "pk": jr.id})
        response = admin_client.post(url, {}, format="json")

        assert response.status_code == status.HTTP_200_OK
        jr.refresh_from_db()
        assert jr.status == WorkspaceJoinRequest.Status.APPROVED
        assert jr.responded_at is not None

        member = WorkspaceMember.objects.get(workspace=admin_workspace, member=create_user)
        assert member.is_active is True
        assert member.role == MEMBER_ROLE

        # last_workspace_id lives on Profile; approval should point there.
        profile.refresh_from_db()
        assert profile.last_workspace_id == admin_workspace.id

    @pytest.mark.django_db
    def test_approve_twice_is_rejected(self, mocker, admin_client, create_user, admin_workspace):
        mocker.patch("pi_dash.app.views.workspace.join_request.track_event.delay")
        jr = WorkspaceJoinRequest.objects.create(
            requester=create_user, workspace=admin_workspace, admin_email="admin@example.com"
        )
        url = reverse("workspace-join-request-approve", kwargs={"slug": admin_workspace.slug, "pk": jr.id})
        admin_client.post(url, {}, format="json")
        response = admin_client.post(url, {}, format="json")
        assert response.status_code == status.HTTP_400_BAD_REQUEST

    @pytest.mark.django_db
    def test_deny_marks_denied_without_membership(self, admin_client, create_user, admin_workspace):
        jr = WorkspaceJoinRequest.objects.create(
            requester=create_user, workspace=admin_workspace, admin_email="admin@example.com"
        )
        url = reverse("workspace-join-request-deny", kwargs={"slug": admin_workspace.slug, "pk": jr.id})
        response = admin_client.post(url, {}, format="json")

        assert response.status_code == status.HTTP_200_OK
        jr.refresh_from_db()
        assert jr.status == WorkspaceJoinRequest.Status.DENIED
        assert not WorkspaceMember.objects.filter(workspace=admin_workspace, member=create_user).exists()

    @pytest.mark.django_db
    def test_non_admin_cannot_approve(self, session_client, create_user, admin_workspace):
        jr = WorkspaceJoinRequest.objects.create(
            requester=create_user, workspace=admin_workspace, admin_email="admin@example.com"
        )
        url = reverse("workspace-join-request-approve", kwargs={"slug": admin_workspace.slug, "pk": jr.id})
        response = session_client.post(url, {}, format="json")
        assert response.status_code == status.HTTP_403_FORBIDDEN
