# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for project-guest reads of the archives.

Guests read the archived issues list, the archived issue detail and the
archived cycles list like the live lists: a guest without
``guest_view_all_features`` sees only the archived issues they created.
Archive writes stay ADMIN/MEMBER-only.
"""

import pytest
from django.utils import timezone
from rest_framework import status as http_status

from pi_dash.db.models import Cycle, Issue, ProjectMember, State, User, WorkspaceMember


@pytest.fixture
def archive_world(db, workspace, project, create_user):
    ProjectMember.objects.get_or_create(project=project, member=create_user, defaults={"role": 20})
    guest = User.objects.create(email="archive-guest@example.com", username="archive-guest")
    WorkspaceMember.objects.create(workspace=workspace, member=guest, role=5)
    ProjectMember.objects.create(project=project, member=guest, role=5, is_active=True)
    done = State.objects.create(name="Done", group="completed", project=project, workspace=workspace)
    now = timezone.now()
    admin_issue = Issue.objects.create(
        name="Archived by admin",
        project=project,
        workspace=workspace,
        state=done,
        archived_at=now,
    )
    guest_issue = Issue.objects.create(
        name="Archived guest issue",
        project=project,
        workspace=workspace,
        state=done,
        archived_at=now,
    )
    # BaseModel.save overwrites created_by from the (absent) request user.
    Issue.objects.filter(pk=admin_issue.pk).update(created_by=create_user)
    Issue.objects.filter(pk=guest_issue.pk).update(created_by=guest)
    cycle = Cycle.objects.create(
        name="Archived cycle",
        project=project,
        workspace=workspace,
        owned_by=create_user,
        archived_at=now,
    )
    return {
        "project": project,
        "guest": guest,
        "admin_issue": admin_issue,
        "guest_issue": guest_issue,
        "cycle": cycle,
    }


def _base(workspace, project):
    return f"/api/workspaces/{workspace.slug}/projects/{project.id}"


def _result_ids(response):
    return {row["id"] for row in response.json()["results"]}


@pytest.mark.contract
class TestArchiveGuestReads:
    @pytest.mark.django_db
    def test_guest_reads_archived_issues_scoped_to_own(self, session_client, workspace, archive_world):
        project = archive_world["project"]
        session_client.force_authenticate(user=archive_world["guest"])

        response = session_client.get(f"{_base(workspace, project)}/archived-issues/")

        assert response.status_code == http_status.HTTP_200_OK
        assert _result_ids(response) == {str(archive_world["guest_issue"].id)}

    @pytest.mark.django_db
    def test_guest_with_view_all_reads_every_archived_issue(self, session_client, workspace, archive_world):
        project = archive_world["project"]
        project.guest_view_all_features = True
        project.save()
        session_client.force_authenticate(user=archive_world["guest"])

        response = session_client.get(f"{_base(workspace, project)}/archived-issues/")

        assert response.status_code == http_status.HTTP_200_OK
        assert _result_ids(response) == {
            str(archive_world["guest_issue"].id),
            str(archive_world["admin_issue"].id),
        }

    @pytest.mark.django_db
    def test_admin_still_reads_every_archived_issue(self, session_client, workspace, archive_world):
        response = session_client.get(f"{_base(workspace, archive_world['project'])}/archived-issues/")

        assert response.status_code == http_status.HTTP_200_OK
        assert _result_ids(response) == {
            str(archive_world["guest_issue"].id),
            str(archive_world["admin_issue"].id),
        }

    @pytest.mark.django_db
    def test_guest_reads_own_archived_issue_detail(self, session_client, workspace, archive_world):
        project = archive_world["project"]
        issue = archive_world["guest_issue"]
        session_client.force_authenticate(user=archive_world["guest"])

        response = session_client.get(f"{_base(workspace, project)}/issues/{issue.id}/archive/")

        assert response.status_code == http_status.HTTP_200_OK
        assert response.json()["id"] == str(issue.id)

    @pytest.mark.django_db
    def test_guest_refused_others_archived_issue_detail(self, session_client, workspace, archive_world):
        project = archive_world["project"]
        issue = archive_world["admin_issue"]
        session_client.force_authenticate(user=archive_world["guest"])

        response = session_client.get(f"{_base(workspace, project)}/issues/{issue.id}/archive/")

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        assert response.json() == {"error": "You are not allowed to view this issue"}

    @pytest.mark.django_db
    def test_guest_with_view_all_reads_others_archived_issue_detail(self, session_client, workspace, archive_world):
        project = archive_world["project"]
        project.guest_view_all_features = True
        project.save()
        issue = archive_world["admin_issue"]
        session_client.force_authenticate(user=archive_world["guest"])

        response = session_client.get(f"{_base(workspace, project)}/issues/{issue.id}/archive/")

        assert response.status_code == http_status.HTTP_200_OK
        assert response.json()["id"] == str(issue.id)

    @pytest.mark.django_db
    def test_guest_reads_archived_cycles(self, session_client, workspace, archive_world):
        project = archive_world["project"]
        session_client.force_authenticate(user=archive_world["guest"])

        response = session_client.get(f"{_base(workspace, project)}/archived-cycles/")

        assert response.status_code == http_status.HTTP_200_OK
        assert [row["id"] for row in response.json()] == [str(archive_world["cycle"].id)]

    @pytest.mark.django_db
    def test_guest_still_refused_archive_writes(self, session_client, workspace, archive_world):
        project = archive_world["project"]
        base = _base(workspace, project)
        session_client.force_authenticate(user=archive_world["guest"])

        unarchive_issue = session_client.delete(f"{base}/issues/{archive_world['guest_issue'].id}/archive/")
        unarchive_cycle = session_client.delete(f"{base}/cycles/{archive_world['cycle'].id}/archive/")

        assert unarchive_issue.status_code == http_status.HTTP_403_FORBIDDEN
        assert unarchive_cycle.status_code == http_status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_non_member_still_refused_archived_reads(self, session_client, workspace, archive_world):
        project = archive_world["project"]
        outsider = User.objects.create(email="archive-outsider@example.com", username="archive-outsider")
        WorkspaceMember.objects.create(workspace=workspace, member=outsider, role=15)
        session_client.force_authenticate(user=outsider)
        base = _base(workspace, project)

        assert session_client.get(f"{base}/archived-issues/").status_code == http_status.HTTP_403_FORBIDDEN
        assert session_client.get(f"{base}/archived-cycles/").status_code == http_status.HTTP_403_FORBIDDEN
