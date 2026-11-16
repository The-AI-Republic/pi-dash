# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for project-guest reads on cycle retrieve (NEWFRONT-1036).

Mirror of PDASHOSS01-327 on web-new-dev: guests could list cycles but got 403
on retrieve, so no guest cycle page rendered. Guests may read a cycle's
detail with the same shape a member gets; every cycle write (create, update,
delete, archive, transfer) stays admin/member-only.
"""

from datetime import timedelta
from unittest.mock import patch
from uuid import uuid4

import pytest
from django.utils import timezone
from rest_framework import status
from rest_framework.test import APIClient

from pi_dash.db.models import Cycle, ProjectMember, User, WorkspaceMember

PERMISSION_DENIED = {"error": "You don't have the required permissions."}
RECENT_VISITED = "pi_dash.app.views.cycle.base.recent_visited_task"
MODEL_ACTIVITY = "pi_dash.app.views.cycle.base.model_activity"


def _cycles_url(slug, project_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/cycles/"


def _cycle_url(slug, project_id, cycle_id):
    return f"{_cycles_url(slug, project_id)}{cycle_id}/"


def _archive_url(slug, project_id, cycle_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/archive/"


def _transfer_url(slug, project_id, cycle_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/transfer-issues/"


@pytest.fixture
def member_user(db):
    return User.objects.create_user(email="cycle-member@example.com", username="cycle-member")


@pytest.fixture
def guest_user(db):
    return User.objects.create_user(email="cycle-guest@example.com", username="cycle-guest")


@pytest.fixture
def outsider_user(db):
    return User.objects.create_user(email="cycle-outsider@example.com", username="cycle-outsider")


@pytest.fixture
def memberships(workspace, project, member_user, guest_user):
    WorkspaceMember.objects.create(workspace=workspace, member=member_user, role=15)
    ProjectMember.objects.create(project=project, member=member_user, role=15, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=guest_user, role=5)
    ProjectMember.objects.create(project=project, member=guest_user, role=5, is_active=True)


@pytest.fixture
def cycle(workspace, project, member_user):
    return Cycle.objects.create(
        name="Guest readable cycle",
        project=project,
        workspace=workspace,
        owned_by=member_user,
        start_date=timezone.now() - timedelta(days=1),
        end_date=timezone.now() + timedelta(days=7),
    )


def _client_for(user):
    client = APIClient()
    client.force_authenticate(user=user)
    return client


@pytest.mark.contract
@pytest.mark.django_db
class TestCycleRetrieveGuestRead:
    @patch(RECENT_VISITED)
    def test_guest_retrieve_matches_member_shape(
        self, recent_visited, workspace, project, memberships, cycle, member_user, guest_user
    ):
        url = _cycle_url(workspace.slug, project.id, cycle.id)
        guest = _client_for(guest_user).get(url)
        member = _client_for(member_user).get(url)

        assert guest.status_code == status.HTTP_200_OK, guest.content
        assert member.status_code == status.HTTP_200_OK, member.content
        assert guest.json() == member.json()
        assert guest.json()["id"] == str(cycle.id)
        assert guest.json()["name"] == "Guest readable cycle"
        assert recent_visited.delay.call_count == 2

    @patch(RECENT_VISITED)
    def test_guest_retrieve_unknown_cycle_is_404(self, recent_visited, workspace, project, memberships, guest_user):
        response = _client_for(guest_user).get(_cycle_url(workspace.slug, project.id, uuid4()))

        assert response.status_code == status.HTTP_404_NOT_FOUND
        assert response.json() == {"error": "Cycle not found"}
        recent_visited.delay.assert_not_called()

    def test_outsider_retrieve_refused(self, workspace, project, memberships, cycle, outsider_user):
        response = _client_for(outsider_user).get(_cycle_url(workspace.slug, project.id, cycle.id))

        assert response.status_code == status.HTTP_403_FORBIDDEN, response.content
        assert response.json() == PERMISSION_DENIED


@pytest.mark.contract
@pytest.mark.django_db
class TestCycleGuestWrites:
    def test_guest_writes_are_refused(self, workspace, project, memberships, cycle, guest_user):
        client = _client_for(guest_user)
        detail = _cycle_url(workspace.slug, project.id, cycle.id)

        responses = [
            client.post(_cycles_url(workspace.slug, project.id), {"name": "guest cycle"}, format="json"),
            client.patch(detail, {"name": "renamed by guest"}, format="json"),
            client.delete(detail),
            client.post(_archive_url(workspace.slug, project.id, cycle.id), {}, format="json"),
            client.post(
                _transfer_url(workspace.slug, project.id, cycle.id),
                {"new_cycle_id": str(cycle.id)},
                format="json",
            ),
        ]

        for response in responses:
            assert response.status_code == status.HTTP_403_FORBIDDEN, response.content
            assert response.json() == PERMISSION_DENIED

        cycle.refresh_from_db()
        assert cycle.name == "Guest readable cycle"
        assert cycle.archived_at is None

    def test_member_writes_still_pass_the_gate(self, workspace, project, memberships, cycle, member_user):
        with patch(MODEL_ACTIVITY):
            response = _client_for(member_user).patch(
                _cycle_url(workspace.slug, project.id, cycle.id),
                {"name": "Renamed by member"},
                format="json",
            )

        assert response.status_code == status.HTTP_200_OK, response.content
        cycle.refresh_from_db()
        assert cycle.name == "Renamed by member"
