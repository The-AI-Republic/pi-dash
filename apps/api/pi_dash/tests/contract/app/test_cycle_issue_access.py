# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Role gates on ``CycleIssueViewSet``.

Project guests read a cycle's work items (list and retrieve) the way they read
the project issues list; every cycle-issue write stays admin/member only.
"""

from datetime import timedelta

import pytest
from django.utils import timezone
from rest_framework import status as http_status
from rest_framework.test import APIClient

from pi_dash.db.models import (
    Cycle,
    CycleIssue,
    Issue,
    ProjectMember,
    User,
    WorkspaceMember,
)

PERMISSION_DENIED = {"error": "You don't have the required permissions."}


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
def cycle_setup(project, create_user, member_of):
    admin, admin_client = member_of("cycle-admin@example.com", 20)
    guest, guest_client = member_of("cycle-guest@example.com", 5)
    cycle = Cycle.objects.create(
        name="Guest Read Cycle",
        project=project,
        workspace=project.workspace,
        owned_by=admin,
        start_date=timezone.now() - timedelta(days=1),
        end_date=timezone.now() + timedelta(days=7),
    )

    def add_issue(name, author):
        issue = Issue.objects.create(name=name, project=project, workspace=project.workspace)
        # The audit save() stamps created_by from the request user; there is none here.
        Issue.objects.filter(pk=issue.pk).update(created_by=author)
        CycleIssue.objects.create(cycle=cycle, issue=issue, project=project, workspace=project.workspace)
        return issue

    admin_issue = add_issue("admin item", admin)
    guest_issue = add_issue("guest item", guest)
    outside_issue = Issue.objects.create(
        name="not in cycle", project=project, workspace=project.workspace, created_by=admin
    )
    base = f"/api/workspaces/{project.workspace.slug}/projects/{project.id}/cycles/{cycle.id}/cycle-issues/"
    return {
        "project": project,
        "cycle": cycle,
        "admin_client": admin_client,
        "guest_client": guest_client,
        "admin_issue": admin_issue,
        "guest_issue": guest_issue,
        "outside_issue": outside_issue,
        "list_url": base,
        "detail_url": lambda issue: f"{base}{issue.id}/",
    }


def _ids(response):
    return sorted(row["id"] for row in response.json()["results"])


@pytest.mark.contract
@pytest.mark.django_db
class TestCycleIssueGuestRead:
    def test_guest_list_matches_member_shape_when_guests_see_all(self, cycle_setup):
        project = cycle_setup["project"]
        project.guest_view_all_features = True
        project.save()

        guest = cycle_setup["guest_client"].get(cycle_setup["list_url"])
        admin = cycle_setup["admin_client"].get(cycle_setup["list_url"])

        assert guest.status_code == http_status.HTTP_200_OK, guest.content
        assert admin.status_code == http_status.HTTP_200_OK, admin.content
        assert guest.json() == admin.json()
        assert _ids(guest) == sorted([str(cycle_setup["admin_issue"].id), str(cycle_setup["guest_issue"].id)])

    def test_guest_list_grouped_and_filtered(self, cycle_setup):
        project = cycle_setup["project"]
        project.guest_view_all_features = True
        project.save()

        params = {"group_by": "priority", "priority": "none"}
        guest = cycle_setup["guest_client"].get(cycle_setup["list_url"], params)
        admin = cycle_setup["admin_client"].get(cycle_setup["list_url"], params)

        assert guest.status_code == http_status.HTTP_200_OK, guest.content
        assert guest.json() == admin.json()

    def test_guest_list_scoped_to_own_items_without_view_all(self, cycle_setup):
        guest = cycle_setup["guest_client"].get(cycle_setup["list_url"])
        admin = cycle_setup["admin_client"].get(cycle_setup["list_url"])

        assert guest.status_code == http_status.HTTP_200_OK, guest.content
        assert _ids(guest) == [str(cycle_setup["guest_issue"].id)]
        assert guest.json()["total_count"] == 1
        assert len(_ids(admin)) == 2

    def test_guest_retrieve(self, cycle_setup):
        project = cycle_setup["project"]
        project.guest_view_all_features = True
        project.save()
        url = cycle_setup["detail_url"](cycle_setup["admin_issue"])

        guest = cycle_setup["guest_client"].get(url)
        listed = cycle_setup["admin_client"].get(cycle_setup["list_url"]).json()["results"]

        assert guest.status_code == http_status.HTTP_200_OK, guest.content
        assert guest.json() == next(row for row in listed if row["id"] == str(cycle_setup["admin_issue"].id))
        assert guest.json()["cycle_id"] == str(cycle_setup["cycle"].id)

    def test_retrieve_outside_cycle_is_404(self, cycle_setup):
        project = cycle_setup["project"]
        project.guest_view_all_features = True
        project.save()

        response = cycle_setup["guest_client"].get(cycle_setup["detail_url"](cycle_setup["outside_issue"]))

        assert response.status_code == http_status.HTTP_404_NOT_FOUND
        assert response.json() == {"error": "The required object does not exist."}

    def test_guest_retrieve_scoped_without_view_all(self, cycle_setup):
        own = cycle_setup["guest_client"].get(cycle_setup["detail_url"](cycle_setup["guest_issue"]))
        other = cycle_setup["guest_client"].get(cycle_setup["detail_url"](cycle_setup["admin_issue"]))

        assert own.status_code == http_status.HTTP_200_OK, own.content
        assert other.status_code == http_status.HTTP_404_NOT_FOUND


@pytest.mark.contract
@pytest.mark.django_db
class TestCycleIssueGuestWrites:
    def test_guest_writes_are_refused(self, cycle_setup):
        client = cycle_setup["guest_client"]
        detail = cycle_setup["detail_url"](cycle_setup["admin_issue"])

        responses = [
            client.post(cycle_setup["list_url"], {"issues": [str(cycle_setup["outside_issue"].id)]}, format="json"),
            client.patch(detail, {"sort_order": 1}, format="json"),
            client.put(detail, {"sort_order": 1}, format="json"),
            client.delete(detail),
        ]

        for response in responses:
            assert response.status_code == http_status.HTTP_403_FORBIDDEN, response.content
            assert response.json() == PERMISSION_DENIED
        assert CycleIssue.objects.filter(cycle=cycle_setup["cycle"]).count() == 2
        assert not CycleIssue.objects.filter(issue=cycle_setup["outside_issue"]).exists()

    def test_member_writes_still_pass_the_gate(self, cycle_setup):
        client = cycle_setup["admin_client"]

        created = client.post(
            cycle_setup["list_url"], {"issues": [str(cycle_setup["outside_issue"].id)]}, format="json"
        )
        removed = client.delete(cycle_setup["detail_url"](cycle_setup["admin_issue"]))

        assert created.status_code == http_status.HTTP_201_CREATED, created.content
        assert removed.status_code == http_status.HTTP_204_NO_CONTENT
