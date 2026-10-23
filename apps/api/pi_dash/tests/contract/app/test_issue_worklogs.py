# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for issue worklog entries (NEWFRONT-672, ISS-206 cloud leg).

Reads are open to every active project member; logging is for project
admins and assignees on live, non-intake items (guests never); edits and
removals are admin-or-author, mirroring comment authorship. Every write
records a ``worklog.activity.*`` row the feed reads through its own key.
"""

import json
from types import SimpleNamespace
from unittest.mock import patch
from uuid import uuid4

import pytest
from rest_framework import status
from rest_framework.test import APIClient

from pi_dash.app.views.issue.worklog import IssueWorklogViewSet
from pi_dash.bgtasks.issue_activities_task import (
    create_worklog_activity,
    delete_worklog_activity,
    issue_activity,
    update_worklog_activity,
)
from pi_dash.db.models import (
    Intake,
    IntakeIssue,
    Issue,
    IssueActivity,
    IssueAssignee,
    IssueWorklog,
    ProjectMember,
    State,
    User,
    WorkspaceMember,
)


def _collection_url(slug, project_id, issue_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/worklogs/"


def _entry_url(slug, project_id, issue_id, entry_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/worklogs/{entry_id}/"


@pytest.fixture
def member_user(db):
    return User.objects.create_user(email="member@example.com", username="member")


@pytest.fixture
def other_member_user(db):
    return User.objects.create_user(email="other@example.com", username="other")


@pytest.fixture
def guest_user(db):
    return User.objects.create_user(email="guest@example.com", username="guest")


@pytest.fixture
def outsider_user(db):
    return User.objects.create_user(email="outsider@example.com", username="outsider")


@pytest.fixture
def memberships(workspace, project, create_user, member_user, other_member_user, guest_user):
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=member_user, role=15)
    ProjectMember.objects.create(project=project, member=member_user, role=15, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=other_member_user, role=15)
    ProjectMember.objects.create(project=project, member=other_member_user, role=15, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=guest_user, role=5)
    ProjectMember.objects.create(project=project, member=guest_user, role=5, is_active=True)


@pytest.fixture
def worklog_state(workspace, project, create_user):
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
def issue(workspace, project, create_user, worklog_state):
    return _make_issue("worklog issue", project, workspace, worklog_state, create_user)


@pytest.fixture
def assigned_issue(issue, member_user):
    IssueAssignee.objects.create(issue=issue, assignee=member_user, project=issue.project, workspace=issue.workspace)
    return issue


def _make_entry(issue, author, minutes=90, logged_on="2026-10-20", note=""):
    entry = IssueWorklog(
        issue=issue,
        project=issue.project,
        workspace=issue.workspace,
        actor=author,
        minutes=minutes,
        logged_on=logged_on,
        note=note,
        created_by=author,
    )
    entry.save(disable_auto_set_user=True)
    return entry


@pytest.fixture(autouse=True)
def no_celery():
    with (
        patch("pi_dash.app.views.issue.worklog.issue_activity") as activity,
        patch("pi_dash.db.mixins.soft_delete_related_objects.delay"),
    ):
        yield activity


def _client_for(user):
    client = APIClient()
    client.force_authenticate(user=user)
    return client


@pytest.mark.contract
class TestWorklogReads:
    @pytest.mark.django_db
    def test_member_lists_entries_newest_first(self, workspace, project, create_user, memberships, assigned_issue):
        first = _make_entry(assigned_issue, create_user, minutes=30, logged_on="2026-10-19")
        second = _make_entry(assigned_issue, create_user, minutes=90, logged_on="2026-10-20")

        response = _client_for(create_user).get(_collection_url(workspace.slug, project.id, assigned_issue.id))

        assert response.status_code == status.HTTP_200_OK
        assert [str(row["id"]) for row in response.data] == [str(second.id), str(first.id)]
        assert response.data[0]["minutes"] == 90
        assert response.data[0]["logged_on"] == "2026-10-20"
        assert str(response.data[0]["actor_detail"]["id"]) == str(create_user.id)

    @pytest.mark.django_db
    def test_guest_lists_entries(self, workspace, project, create_user, guest_user, memberships, issue):
        _make_entry(issue, create_user)

        response = _client_for(guest_user).get(_collection_url(workspace.slug, project.id, issue.id))

        assert response.status_code == status.HTTP_200_OK
        assert len(response.data) == 1

    @pytest.mark.django_db
    def test_outsider_lists_nothing(self, workspace, project, create_user, outsider_user, memberships, issue):
        _make_entry(issue, create_user)

        response = _client_for(outsider_user).get(_collection_url(workspace.slug, project.id, issue.id))

        assert response.status_code == status.HTTP_200_OK
        assert response.data == []

    @pytest.mark.django_db
    def test_member_retrieves_single_entry(self, workspace, project, create_user, memberships, issue):
        entry = _make_entry(issue, create_user, note="Deep work")

        response = _client_for(create_user).get(_entry_url(workspace.slug, project.id, issue.id, entry.id))

        assert response.status_code == status.HTTP_200_OK
        assert str(response.data["id"]) == str(entry.id)
        assert response.data["note"] == "Deep work"

    @pytest.mark.django_db
    def test_outsider_retrieve_is_not_found(self, workspace, project, create_user, outsider_user, memberships, issue):
        entry = _make_entry(issue, create_user)

        response = _client_for(outsider_user).get(_entry_url(workspace.slug, project.id, issue.id, entry.id))

        assert response.status_code == status.HTTP_404_NOT_FOUND


@pytest.mark.contract
class TestWorklogCreate:
    @pytest.mark.django_db
    def test_assigned_member_logs_time(self, workspace, project, member_user, memberships, assigned_issue, no_celery):
        response = _client_for(member_user).post(
            _collection_url(workspace.slug, project.id, assigned_issue.id),
            {"minutes": 90, "logged_on": "2026-10-20", "note": "Deep work"},
            format="json",
        )

        assert response.status_code == status.HTTP_201_CREATED
        assert response.data["minutes"] == 90
        assert response.data["logged_on"] == "2026-10-20"
        assert response.data["note"] == "Deep work"
        assert str(response.data["actor"]) == str(member_user.id)
        no_celery.delay.assert_called_once()
        assert no_celery.delay.call_args.kwargs["type"] == "worklog.activity.created"

    @pytest.mark.django_db
    def test_admin_logs_time_without_assignment(self, workspace, project, create_user, memberships, issue, no_celery):
        response = _client_for(create_user).post(
            _collection_url(workspace.slug, project.id, issue.id),
            {"minutes": 30, "logged_on": "2026-10-20"},
            format="json",
        )

        assert response.status_code == status.HTTP_201_CREATED
        assert response.data["note"] == ""
        no_celery.delay.assert_called_once()

    @pytest.mark.django_db
    def test_unassigned_member_is_refused(
        self, workspace, project, other_member_user, memberships, assigned_issue, no_celery
    ):
        response = _client_for(other_member_user).post(
            _collection_url(workspace.slug, project.id, assigned_issue.id),
            {"minutes": 30, "logged_on": "2026-10-20"},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN
        no_celery.delay.assert_not_called()

    @pytest.mark.django_db
    def test_guest_is_refused_even_when_assigned(self, workspace, project, guest_user, memberships, issue, no_celery):
        IssueAssignee.objects.create(issue=issue, assignee=guest_user, project=issue.project, workspace=issue.workspace)

        response = _client_for(guest_user).post(
            _collection_url(workspace.slug, project.id, issue.id),
            {"minutes": 30, "logged_on": "2026-10-20"},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN
        no_celery.delay.assert_not_called()

    @pytest.mark.django_db
    def test_outsider_is_refused(self, workspace, project, outsider_user, memberships, issue, no_celery):
        response = _client_for(outsider_user).post(
            _collection_url(workspace.slug, project.id, issue.id),
            {"minutes": 30, "logged_on": "2026-10-20"},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN
        no_celery.delay.assert_not_called()

    @pytest.mark.django_db
    def test_intake_issue_is_refused(
        self, workspace, project, create_user, member_user, memberships, assigned_issue, no_celery
    ):
        intake = Intake.objects.create(name="Requests", project=project, workspace=workspace, created_by=create_user)
        IntakeIssue.objects.create(intake=intake, issue=assigned_issue, project=project, workspace=workspace)

        response = _client_for(member_user).post(
            _collection_url(workspace.slug, project.id, assigned_issue.id),
            {"minutes": 30, "logged_on": "2026-10-20"},
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        no_celery.delay.assert_not_called()

    @pytest.mark.django_db
    def test_invalid_minutes_are_refused(self, workspace, project, member_user, memberships, assigned_issue, no_celery):
        client = _client_for(member_user)
        url = _collection_url(workspace.slug, project.id, assigned_issue.id)

        for payload in (
            {"minutes": 0, "logged_on": "2026-10-20"},
            {"minutes": -30, "logged_on": "2026-10-20"},
            {"logged_on": "2026-10-20"},
            {"minutes": 30, "logged_on": "soon"},
            {"minutes": 30},
        ):
            response = client.post(url, payload, format="json")
            assert response.status_code == status.HTTP_400_BAD_REQUEST

        no_celery.delay.assert_not_called()

    @pytest.mark.django_db
    def test_actor_is_forced_to_the_caller(
        self, workspace, project, member_user, other_member_user, memberships, assigned_issue
    ):
        response = _client_for(member_user).post(
            _collection_url(workspace.slug, project.id, assigned_issue.id),
            {"minutes": 30, "logged_on": "2026-10-20", "actor": str(other_member_user.id)},
            format="json",
        )

        assert response.status_code == status.HTTP_201_CREATED
        assert str(response.data["actor"]) == str(member_user.id)


@pytest.mark.contract
class TestWorklogUpdateDelete:
    @pytest.mark.django_db
    def test_author_edits_own_entry(
        self, workspace, project, create_user, member_user, memberships, assigned_issue, no_celery
    ):
        entry = _make_entry(assigned_issue, member_user)

        response = _client_for(member_user).patch(
            _entry_url(workspace.slug, project.id, assigned_issue.id, entry.id),
            {"minutes": 120, "note": "Still going"},
            format="json",
        )

        assert response.status_code == status.HTTP_200_OK
        assert response.data["minutes"] == 120
        assert response.data["note"] == "Still going"
        no_celery.delay.assert_called_once()
        assert no_celery.delay.call_args.kwargs["type"] == "worklog.activity.updated"

    @pytest.mark.django_db
    def test_admin_edits_foreign_entry(
        self, workspace, project, create_user, member_user, memberships, assigned_issue, no_celery
    ):
        entry = _make_entry(assigned_issue, member_user)

        response = _client_for(create_user).patch(
            _entry_url(workspace.slug, project.id, assigned_issue.id, entry.id),
            {"note": "Admin correction"},
            format="json",
        )

        assert response.status_code == status.HTTP_200_OK
        assert response.data["note"] == "Admin correction"
        no_celery.delay.assert_called_once()

    @pytest.mark.django_db
    def test_other_member_cannot_edit(
        self,
        workspace,
        project,
        member_user,
        other_member_user,
        memberships,
        assigned_issue,
        no_celery,
    ):
        entry = _make_entry(assigned_issue, member_user)

        response = _client_for(other_member_user).patch(
            _entry_url(workspace.slug, project.id, assigned_issue.id, entry.id),
            {"note": "Hijack"},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN
        no_celery.delay.assert_not_called()

    @pytest.mark.django_db
    def test_guest_cannot_edit(self, workspace, project, create_user, guest_user, memberships, issue, no_celery):
        entry = _make_entry(issue, create_user)

        response = _client_for(guest_user).patch(
            _entry_url(workspace.slug, project.id, issue.id, entry.id),
            {"note": "Hijack"},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN
        no_celery.delay.assert_not_called()

    @pytest.mark.django_db
    def test_author_removes_own_entry(self, workspace, project, member_user, memberships, assigned_issue, no_celery):
        entry = _make_entry(assigned_issue, member_user)

        response = _client_for(member_user).delete(_entry_url(workspace.slug, project.id, assigned_issue.id, entry.id))

        assert response.status_code == status.HTTP_204_NO_CONTENT
        assert not IssueWorklog.objects.filter(id=entry.id).exists()
        no_celery.delay.assert_called_once()
        assert no_celery.delay.call_args.kwargs["type"] == "worklog.activity.deleted"

    @pytest.mark.django_db
    def test_admin_removes_foreign_entry(
        self, workspace, project, create_user, member_user, memberships, assigned_issue, no_celery
    ):
        entry = _make_entry(assigned_issue, member_user)

        response = _client_for(create_user).delete(_entry_url(workspace.slug, project.id, assigned_issue.id, entry.id))

        assert response.status_code == status.HTTP_204_NO_CONTENT
        assert not IssueWorklog.objects.filter(id=entry.id).exists()

    @pytest.mark.django_db
    def test_other_member_cannot_remove(
        self,
        workspace,
        project,
        member_user,
        other_member_user,
        memberships,
        assigned_issue,
        no_celery,
    ):
        entry = _make_entry(assigned_issue, member_user)

        response = _client_for(other_member_user).delete(
            _entry_url(workspace.slug, project.id, assigned_issue.id, entry.id)
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN
        assert IssueWorklog.objects.filter(id=entry.id).exists()
        no_celery.delay.assert_not_called()


@pytest.mark.contract
class TestWorklogActivities:
    @pytest.mark.django_db
    def test_create_handler_writes_field_verb_and_minutes(self, workspace, project, create_user, issue):
        rows = []

        create_worklog_activity(
            requested_data=json.dumps({"id": "entry-id", "minutes": 90}),
            current_instance=None,
            issue_id=str(issue.id),
            project_id=str(project.id),
            workspace_id=str(workspace.id),
            actor_id=str(create_user.id),
            issue_activities=rows,
            epoch=123,
        )

        assert len(rows) == 1
        assert rows[0].field == "worklog"
        assert rows[0].verb == "created"
        assert rows[0].new_value == "90"
        assert rows[0].new_identifier == "entry-id"

    @pytest.mark.django_db
    def test_update_handler_carries_before_and_after_minutes(self, workspace, project, create_user, issue):
        rows = []

        update_worklog_activity(
            requested_data=json.dumps({"minutes": 120}),
            current_instance=json.dumps({"minutes": 90}),
            issue_id=str(issue.id),
            project_id=str(project.id),
            workspace_id=str(workspace.id),
            actor_id=str(create_user.id),
            issue_activities=rows,
            epoch=123,
        )

        assert len(rows) == 1
        assert rows[0].field == "worklog"
        assert rows[0].verb == "updated"
        assert rows[0].old_value == "90"
        assert rows[0].new_value == "120"

    @pytest.mark.django_db
    def test_update_handler_falls_back_when_minutes_untouched(self, workspace, project, create_user, issue):
        rows = []

        update_worklog_activity(
            requested_data=json.dumps({"note": "Typo fix"}),
            current_instance=json.dumps({"minutes": 90}),
            issue_id=str(issue.id),
            project_id=str(project.id),
            workspace_id=str(workspace.id),
            actor_id=str(create_user.id),
            issue_activities=rows,
            epoch=123,
        )

        assert rows[0].old_value == "90"
        assert rows[0].new_value == "90"

    @pytest.mark.django_db
    def test_delete_handler_writes_a_bare_row(self, workspace, project, create_user, issue):
        rows = []

        delete_worklog_activity(
            requested_data=json.dumps({"worklog_id": "entry-id"}),
            current_instance=json.dumps({"minutes": 90}),
            issue_id=str(issue.id),
            project_id=str(project.id),
            workspace_id=str(workspace.id),
            actor_id=str(create_user.id),
            issue_activities=rows,
            epoch=123,
        )

        assert len(rows) == 1
        assert rows[0].field == "worklog"
        assert rows[0].verb == "deleted"

    @pytest.mark.django_db
    def test_logged_entry_surfaces_in_history(self, workspace, project, create_user, memberships, issue):
        entry = _make_entry(issue, create_user, minutes=45)
        rows = []

        create_worklog_activity(
            requested_data=json.dumps({"id": str(entry.id), "minutes": entry.minutes}),
            current_instance=None,
            issue_id=str(issue.id),
            project_id=str(project.id),
            workspace_id=str(workspace.id),
            actor_id=str(create_user.id),
            issue_activities=rows,
            epoch=123,
        )
        IssueActivity.objects.bulk_create(rows)

        history = IssueActivity.objects.filter(issue=issue, field="worklog")
        assert history.count() == 1
        assert history.first().new_value == "45"


@pytest.mark.contract
class TestWorklogUnits:
    @pytest.mark.django_db
    def test_entry_string_names_minutes_day_and_issue(self, workspace, project, create_user, issue):
        entry = _make_entry(issue, create_user, minutes=90, logged_on="2026-10-20")

        assert str(entry) == f"90m on 2026-10-20 ({entry.issue_id})"

    @pytest.mark.django_db
    def test_can_log_refuses_without_membership(self, workspace, project, outsider_user):
        view = IssueWorklogViewSet()
        view.request = SimpleNamespace(user=outsider_user)

        assert view._can_log(workspace.slug, str(project.id), None) is False
        assert view._can_log(workspace.slug, str(uuid4()), None) is False

    @pytest.mark.django_db
    def test_task_dispatches_created_type_to_a_feed_row(self, workspace, project, create_user, issue):
        entry = _make_entry(issue, create_user, minutes=45)

        with patch("pi_dash.bgtasks.issue_activities_task.notifications") as notifications:
            issue_activity(
                type="worklog.activity.created",
                requested_data=json.dumps({"id": str(entry.id), "minutes": entry.minutes}),
                current_instance=None,
                issue_id=str(issue.id),
                actor_id=str(create_user.id),
                project_id=str(project.id),
                epoch=123,
                notification=True,
                origin="http://localhost",
            )

        row = IssueActivity.objects.get(issue=issue, field="worklog")
        assert row.verb == "created"
        assert row.new_value == "45"
        assert str(row.new_identifier) == str(entry.id)
        notifications.delay.assert_called_once()
