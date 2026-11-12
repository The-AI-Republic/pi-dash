# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Role gates on ``CycleViewSet.retrieve``.

Project guests read a cycle's detail the way they read the cycle list; every
write on the cycle stays member+ (destroy: admin or creator).
"""

from datetime import timedelta
from unittest.mock import patch

import pytest
from django.utils import timezone
from rest_framework import status as http_status
from rest_framework.test import APIClient

from pi_dash.db.models import Cycle, CycleIssue, Issue, ProjectMember, User, WorkspaceMember

PERMISSION_DENIED = {"error": "You don't have the required permissions."}


@pytest.fixture(autouse=True)
def recent_visited():
    with patch("pi_dash.app.views.cycle.base.recent_visited_task") as task:
        yield task


@pytest.fixture(autouse=True)
def model_activity():
    with patch("pi_dash.app.views.cycle.base.model_activity") as task:
        yield task


@pytest.fixture
def member_of(project):
    def _member(email, role):
        user = User.objects.create(email=email, username=email.split("@")[0], first_name=email.split("@")[0])
        WorkspaceMember.objects.create(workspace=project.workspace, member=user, role=role, is_active=True)
        ProjectMember.objects.create(project=project, member=user, role=role, is_active=True)
        client = APIClient()
        client.force_authenticate(user=user)
        return user, client

    return _member


@pytest.fixture
def cycle_setup(project, member_of):
    admin, admin_client = member_of("retrieve-admin@example.com", 20)
    _, member_client = member_of("retrieve-member@example.com", 15)
    guest, guest_client = member_of("retrieve-guest@example.com", 5)

    def make_cycle(name):
        return Cycle.objects.create(
            name=name,
            project=project,
            workspace=project.workspace,
            owned_by=admin,
            start_date=timezone.now() - timedelta(days=1),
            end_date=timezone.now() + timedelta(days=7),
        )

    cycle = make_cycle("Guest Retrieve Cycle")
    other_cycle = make_cycle("Transfer Target")
    base = f"/api/workspaces/{project.workspace.slug}/projects/{project.id}/cycles/"
    return {
        "project": project,
        "guest": guest,
        "cycle": cycle,
        "other_cycle": other_cycle,
        "admin_client": admin_client,
        "member_client": member_client,
        "guest_client": guest_client,
        "list_url": base,
        "detail_url": f"{base}{cycle.id}/",
        "archive_url": f"{base}{cycle.id}/archive/",
        "transfer_url": f"{base}{cycle.id}/transfer-issues/",
    }


@pytest.mark.contract
@pytest.mark.django_db
class TestCycleRetrieveGuestRead:
    def test_guest_retrieve_matches_member_shape(self, cycle_setup):
        guest = cycle_setup["guest_client"].get(cycle_setup["detail_url"])
        member = cycle_setup["member_client"].get(cycle_setup["detail_url"])

        assert guest.status_code == http_status.HTTP_200_OK, guest.content
        assert member.status_code == http_status.HTTP_200_OK, member.content
        assert guest.json() == member.json()
        assert guest.json()["id"] == str(cycle_setup["cycle"].id)

    def test_guest_retrieve_is_not_narrowed_without_view_all(self, cycle_setup):
        """The cycle list is not narrowed for guests, so neither is the detail."""
        project = cycle_setup["project"]
        project.guest_view_all_features = False
        project.save()
        issue = Issue.objects.create(name="admin item", project=project, workspace=project.workspace)
        CycleIssue.objects.create(cycle=cycle_setup["cycle"], issue=issue, project=project, workspace=project.workspace)

        guest = cycle_setup["guest_client"].get(cycle_setup["detail_url"])
        member = cycle_setup["member_client"].get(cycle_setup["detail_url"])

        assert guest.status_code == http_status.HTTP_200_OK, guest.content
        assert guest.json() == member.json()
        assert guest.json()["total_issues"] == 1

    def test_guest_retrieve_records_the_visit_as_the_guest(self, cycle_setup, recent_visited):
        response = cycle_setup["guest_client"].get(cycle_setup["detail_url"])

        assert response.status_code == http_status.HTTP_200_OK, response.content
        recent_visited.delay.assert_called_once()
        assert recent_visited.delay.call_args.kwargs["user_id"] == cycle_setup["guest"].id

    def test_guest_retrieve_missing_cycle_is_404(self, cycle_setup):
        response = cycle_setup["guest_client"].get(f"{cycle_setup['list_url']}{cycle_setup['project'].id}/")

        assert response.status_code == http_status.HTTP_404_NOT_FOUND
        assert response.json() == {"error": "Cycle not found"}

    def test_guest_retrieve_archived_cycle_is_404(self, cycle_setup):
        Cycle.objects.filter(pk=cycle_setup["cycle"].pk).update(archived_at=timezone.now())

        response = cycle_setup["guest_client"].get(cycle_setup["detail_url"])

        assert response.status_code == http_status.HTTP_404_NOT_FOUND
        assert response.json() == {"error": "Cycle not found"}

    def test_non_member_retrieve_is_refused(self, cycle_setup):
        outsider = User.objects.create(email="retrieve-outsider@example.com", username="retrieve-outsider")
        client = APIClient()
        client.force_authenticate(user=outsider)

        response = client.get(cycle_setup["detail_url"])

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        assert response.json() == PERMISSION_DENIED


@pytest.mark.contract
@pytest.mark.django_db
class TestCycleGuestWrites:
    def test_guest_writes_are_refused(self, cycle_setup):
        client = cycle_setup["guest_client"]
        cycle = cycle_setup["cycle"]

        responses = {
            "create": client.post(cycle_setup["list_url"], {"name": "Guest cycle"}, format="json"),
            "update": client.patch(cycle_setup["detail_url"], {"name": "Renamed"}, format="json"),
            "destroy": client.delete(cycle_setup["detail_url"]),
            "archive": client.post(cycle_setup["archive_url"]),
            "unarchive": client.delete(cycle_setup["archive_url"]),
            "transfer": client.post(
                cycle_setup["transfer_url"], {"new_cycle_id": str(cycle_setup["other_cycle"].id)}, format="json"
            ),
        }

        for action, response in responses.items():
            assert response.status_code == http_status.HTTP_403_FORBIDDEN, (action, response.content)
            assert response.json() == PERMISSION_DENIED, action
        cycle.refresh_from_db()
        assert cycle.name == "Guest Retrieve Cycle"
        assert cycle.archived_at is None
        assert cycle.deleted_at is None
        assert Cycle.objects.filter(project=cycle_setup["project"]).count() == 2

    def test_member_retrieve_and_update_still_pass_the_gate(self, cycle_setup):
        client = cycle_setup["member_client"]

        read = client.get(cycle_setup["detail_url"])
        updated = client.patch(cycle_setup["detail_url"], {"name": "Renamed"}, format="json")

        assert read.status_code == http_status.HTTP_200_OK, read.content
        assert updated.status_code == http_status.HTTP_200_OK, updated.content
