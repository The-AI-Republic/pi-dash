# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the epics collection (NEWFRONT-984, ISS-232 cloud leg).

There is no Epic model: an epic is an issue whose type carries
``is_epic``. The epics views subclass the issue views, so reads, writes,
permissions and activities mirror the issues collection exactly; the
only epic-specific behavior is the boundary (epic-typed rows in,
everything else 404) and the forced epic type on create.
"""

from unittest.mock import patch
from uuid import uuid4

import pytest
from rest_framework import status
from rest_framework.test import APIClient

from pi_dash.db.models import (
    Issue,
    IssueType,
    ProjectMember,
    State,
    User,
    WorkspaceMember,
)
from pi_dash.db.models.issue_type import ProjectIssueType


def _collection_url(slug, project_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/epics/"


def _entry_url(slug, project_id, epic_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/epics/{epic_id}/"


def _children_url(slug, project_id, epic_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/epics/{epic_id}/issues/"


def _issue_url(slug, project_id, issue_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/"


def _sub_issues_url(slug, project_id, issue_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/sub-issues/"


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
def todo_state(workspace, project, create_user):
    return State.objects.create(
        name="Todo",
        project=project,
        workspace=workspace,
        group="unstarted",
        default=True,
        created_by=create_user,
    )


@pytest.fixture
def epic_type(workspace, project, create_user):
    issue_type = IssueType.objects.create(
        name="Epic",
        workspace=workspace,
        is_epic=True,
        created_by=create_user,
    )
    ProjectIssueType.objects.create(
        project=project,
        workspace=workspace,
        issue_type=issue_type,
        level=0,
    )
    return issue_type


def _make_issue(name, project, workspace, state, creator, issue_type=None):
    # BaseModel.save() overwrites created_by from the request user (None in
    # tests), so save explicitly to keep the creator the scoping relies on.
    issue = Issue(
        name=name,
        description_html="<p>body</p>",
        description_stripped="body",
        project=project,
        workspace=workspace,
        state=state,
        type=issue_type,
        created_by=creator,
    )
    issue.save(disable_auto_set_user=True)
    return issue


@pytest.fixture
def epic(workspace, project, create_user, todo_state, epic_type):
    return _make_issue("epic one", project, workspace, todo_state, create_user, epic_type)


@pytest.fixture
def plain_issue(workspace, project, create_user, todo_state):
    return _make_issue("plain one", project, workspace, todo_state, create_user)


@pytest.fixture(autouse=True)
def no_celery():
    with (
        patch("pi_dash.app.views.issue.epics.issue_activity") as activity,
        patch("pi_dash.app.views.issue.epics.model_activity"),
        patch("pi_dash.app.views.issue.epics.issue_description_version_task"),
        patch("pi_dash.app.views.issue.base.issue_activity"),
        patch("pi_dash.app.views.issue.base.model_activity"),
        patch("pi_dash.app.views.issue.base.issue_description_version_task"),
        patch("pi_dash.app.views.issue.base.recent_visited_task"),
        patch("pi_dash.app.views.issue.sub_issue.issue_activity"),
        patch("pi_dash.db.mixins.soft_delete_related_objects.delay"),
    ):
        yield activity


def _client_for(user):
    client = APIClient()
    client.force_authenticate(user=user)
    return client


@pytest.mark.contract
class TestEpicCreate:
    @pytest.mark.django_db
    def test_member_creates_epic_with_forced_type(
        self, workspace, project, member_user, memberships, todo_state, epic_type, no_celery
    ):
        response = _client_for(member_user).post(
            _collection_url(workspace.slug, project.id),
            {"name": "Shiny epic", "state_id": str(todo_state.id)},
            format="json",
        )

        assert response.status_code == status.HTTP_201_CREATED
        stored = Issue.objects.get(pk=response.data["id"])
        assert stored.type_id == epic_type.id
        assert stored.name == "Shiny epic"
        no_celery.delay.assert_called_once()
        assert no_celery.delay.call_args.kwargs["type"] == "issue.activity.created"

    @pytest.mark.django_db
    def test_create_without_project_epic_type_is_refused(
        self, workspace, project, member_user, memberships
    ):
        response = _client_for(member_user).post(
            _collection_url(workspace.slug, project.id),
            {"name": "Typeless epic"},
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert Issue.objects.filter(name="Typeless epic").count() == 0

    @pytest.mark.django_db
    def test_guest_cannot_create(self, workspace, project, guest_user, memberships, epic_type):
        response = _client_for(guest_user).post(
            _collection_url(workspace.slug, project.id),
            {"name": "Guest epic"},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN


@pytest.mark.contract
class TestEpicReads:
    @pytest.mark.django_db
    def test_list_returns_only_epics(
        self, workspace, project, member_user, memberships, epic, plain_issue
    ):
        response = _client_for(member_user).get(_collection_url(workspace.slug, project.id))

        assert response.status_code == status.HTTP_200_OK
        assert [str(row["id"]) for row in response.data["results"]] == [str(epic.id)]

    @pytest.mark.django_db
    def test_detail_carries_epic_marker(self, workspace, project, member_user, memberships, epic):
        response = _client_for(member_user).get(_entry_url(workspace.slug, project.id, epic.id))

        assert response.status_code == status.HTTP_200_OK
        assert response.data["is_epic"] is True
        assert response.data["name"] == "epic one"

    @pytest.mark.django_db
    def test_plain_issue_is_not_found(
        self, workspace, project, member_user, memberships, plain_issue, epic_type
    ):
        response = _client_for(member_user).get(
            _entry_url(workspace.slug, project.id, plain_issue.id)
        )

        assert response.status_code == status.HTTP_404_NOT_FOUND

    @pytest.mark.django_db
    def test_guest_lists_but_does_not_retrieve(self, workspace, project, guest_user, memberships, epic):
        # Inherited verbatim from the issues detail: guests list, but the
        # creator check refuses another member's row.
        client = _client_for(guest_user)
        listed = client.get(_collection_url(workspace.slug, project.id))
        retrieved = client.get(_entry_url(workspace.slug, project.id, epic.id))

        assert listed.status_code == status.HTTP_200_OK
        assert retrieved.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_outsider_is_refused(self, workspace, project, outsider_user, memberships, epic):
        response = _client_for(outsider_user).get(_collection_url(workspace.slug, project.id))

        assert response.status_code == status.HTTP_403_FORBIDDEN


@pytest.mark.contract
class TestEpicUpdate:
    @pytest.mark.django_db
    def test_member_renames_epic(self, workspace, project, member_user, memberships, epic):
        response = _client_for(member_user).patch(
            _entry_url(workspace.slug, project.id, epic.id),
            {"name": "Renamed epic"},
            format="json",
        )

        assert response.status_code == status.HTTP_204_NO_CONTENT
        epic.refresh_from_db()
        assert epic.name == "Renamed epic"

    @pytest.mark.django_db
    def test_type_change_is_refused(self, workspace, project, member_user, memberships, epic):
        response = _client_for(member_user).patch(
            _entry_url(workspace.slug, project.id, epic.id),
            {"type": str(uuid4())},
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        epic.refresh_from_db()
        assert epic.type_id is not None

    @pytest.mark.django_db
    def test_plain_issue_is_not_found(
        self, workspace, project, member_user, memberships, plain_issue, epic_type
    ):
        response = _client_for(member_user).patch(
            _entry_url(workspace.slug, project.id, plain_issue.id),
            {"name": "Hacked"},
            format="json",
        )

        assert response.status_code == status.HTTP_404_NOT_FOUND
        plain_issue.refresh_from_db()
        assert plain_issue.name == "plain one"


@pytest.mark.contract
class TestEpicDelete:
    @pytest.mark.django_db
    def test_admin_deletes_epic(self, workspace, project, create_user, memberships, epic):
        response = _client_for(create_user).delete(_entry_url(workspace.slug, project.id, epic.id))

        assert response.status_code == status.HTTP_204_NO_CONTENT
        assert not Issue.objects.filter(pk=epic.id).exists()

    @pytest.mark.django_db
    def test_plain_issue_is_not_found(
        self, workspace, project, create_user, memberships, plain_issue, epic_type
    ):
        response = _client_for(create_user).delete(
            _entry_url(workspace.slug, project.id, plain_issue.id)
        )

        assert response.status_code == status.HTTP_404_NOT_FOUND
        assert Issue.objects.filter(pk=plain_issue.id).exists()


@pytest.mark.contract
class TestIssueReadTypeMarkers:
    """type_id/is_epic on the issues reads (NEWFRONT-984 Part 4b, ISS-232/234/235).

    The cloud relation editor routes to the epic dialog on the detail
    marker, the cloud type filter scopes sub-issue rows on their type,
    and the detail badge names the type — all three read the issues
    collection, not the epics one.
    """

    @pytest.mark.django_db
    def test_epic_detail_carries_marker_and_type(
        self, workspace, project, member_user, memberships, epic, epic_type
    ):
        response = _client_for(member_user).get(_issue_url(workspace.slug, project.id, epic.id))

        assert response.status_code == status.HTTP_200_OK
        assert response.data["is_epic"] is True
        assert str(response.data["type_id"]) == str(epic_type.id)

    @pytest.mark.django_db
    def test_plain_detail_answers_untyped(
        self, workspace, project, member_user, memberships, plain_issue
    ):
        response = _client_for(member_user).get(
            _issue_url(workspace.slug, project.id, plain_issue.id)
        )

        assert response.status_code == status.HTTP_200_OK
        assert response.data["is_epic"] is False
        assert response.data["type_id"] is None

    @pytest.mark.django_db
    def test_typed_non_epic_detail_answers_typed_not_epic(
        self, workspace, project, create_user, member_user, memberships, todo_state
    ):
        bug_type = IssueType.objects.create(
            name="Bug", workspace=workspace, is_epic=False, created_by=create_user
        )
        issue = _make_issue("bug one", project, workspace, todo_state, create_user, bug_type)

        response = _client_for(member_user).get(_issue_url(workspace.slug, project.id, issue.id))

        assert response.status_code == status.HTTP_200_OK
        assert response.data["is_epic"] is False
        assert str(response.data["type_id"]) == str(bug_type.id)

    @pytest.mark.django_db
    def test_sub_issue_rows_carry_type_id(
        self, workspace, project, create_user, member_user, memberships, todo_state
    ):
        bug_type = IssueType.objects.create(
            name="Bug", workspace=workspace, is_epic=False, created_by=create_user
        )
        parent = _make_issue("parent one", project, workspace, todo_state, create_user)
        child = _make_issue("child one", project, workspace, todo_state, create_user, bug_type)
        child.parent = parent
        child.save(disable_auto_set_user=True)

        response = _client_for(member_user).get(
            _sub_issues_url(workspace.slug, project.id, parent.id)
        )

        assert response.status_code == status.HTTP_200_OK
        rows = {str(row["id"]): row for row in response.data["sub_issues"]}
        assert str(rows[str(child.id)]["type_id"]) == str(bug_type.id)


@pytest.mark.contract
class TestEpicWorkItems:
    @pytest.mark.django_db
    def test_attach_and_list_children(
        self, workspace, project, member_user, memberships, epic, plain_issue
    ):
        client = _client_for(member_user)
        attached = client.post(
            _children_url(workspace.slug, project.id, epic.id),
            {"sub_issue_ids": [str(plain_issue.id)]},
            format="json",
        )

        assert attached.status_code == status.HTTP_200_OK
        listed = client.get(_children_url(workspace.slug, project.id, epic.id))
        assert listed.status_code == status.HTTP_200_OK
        assert [str(row["id"]) for row in listed.data["sub_issues"]] == [str(plain_issue.id)]

    @pytest.mark.django_db
    def test_children_of_plain_issue_are_not_found(
        self, workspace, project, member_user, memberships, plain_issue, epic_type
    ):
        client = _client_for(member_user)
        listed = client.get(_children_url(workspace.slug, project.id, plain_issue.id))
        attached = client.post(
            _children_url(workspace.slug, project.id, plain_issue.id),
            {"sub_issue_ids": [str(plain_issue.id)]},
            format="json",
        )

        assert listed.status_code == status.HTTP_404_NOT_FOUND
        assert attached.status_code == status.HTTP_404_NOT_FOUND
