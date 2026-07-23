# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the workspace-level "all issues" endpoint
(``global-view-issues``).

The endpoint historically returned a flat list only. These tests pin the
contract that it now honours ``group_by`` and returns a grouped response
(``results`` keyed by group), matching the project issues endpoint, so the
web app can render the list layout at workspace scope.
"""

import pytest
from django.urls import reverse
from rest_framework import status as http_status

from pi_dash.db.models import (
    Issue,
    ProjectMember,
    State,
)


@pytest.fixture
def view_project(workspace, create_user):
    """The workspace default project with ``create_user`` as an active member.

    The permission filter on the workspace issues endpoint only returns
    issues from projects the requesting user is an active member of, so the
    membership row is required for the issues to be visible.
    """
    from pi_dash.db.models.project import Project

    project = Project.objects.get(workspace=workspace, identifier="DEF")
    ProjectMember.objects.get_or_create(
        project=project,
        member=create_user,
        defaults={"role": 20, "is_active": True},
    )
    return project


@pytest.fixture
def states(view_project, create_user):
    todo = State.objects.create(
        name="Todo",
        project=view_project,
        workspace=view_project.workspace,
        group="unstarted",
        default=True,
        created_by=create_user,
    )
    done = State.objects.create(
        name="Done",
        project=view_project,
        workspace=view_project.workspace,
        group="completed",
        created_by=create_user,
    )
    return todo, done


@pytest.fixture
def view_issues(view_project, create_user, states):
    todo, done = states
    Issue.objects.create(
        name="Issue in Todo",
        project=view_project,
        workspace=view_project.workspace,
        state=todo,
        created_by=create_user,
    )
    Issue.objects.create(
        name="Issue in Done",
        project=view_project,
        workspace=view_project.workspace,
        state=done,
        created_by=create_user,
    )
    return todo, done


def _url(slug):
    return reverse("global-view-issues", kwargs={"slug": slug})


@pytest.mark.contract
class TestWorkspaceViewIssuesGrouping:
    @pytest.mark.django_db
    def test_ungrouped_returns_flat_list(self, session_client, workspace, view_issues):
        response = session_client.get(_url(workspace.slug))

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["grouped_by"] is None
        assert isinstance(response.data["results"], list)
        assert len(response.data["results"]) == 2

    @pytest.mark.django_db
    def test_group_by_state_returns_grouped_dict(self, session_client, workspace, view_issues):
        todo, done = view_issues

        response = session_client.get(_url(workspace.slug) + "?group_by=state_id")

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["grouped_by"] == "state_id"
        results = response.data["results"]
        # Grouped responses key results by the group value (here, state id).
        assert isinstance(results, dict)
        assert str(todo.id) in results
        assert str(done.id) in results
        assert results[str(todo.id)]["results"][0]["name"] == "Issue in Todo"
        assert results[str(done.id)]["results"][0]["name"] == "Issue in Done"

    @pytest.mark.django_db
    def test_group_by_priority_returns_grouped_dict(self, session_client, workspace, view_issues):
        response = session_client.get(_url(workspace.slug) + "?group_by=priority")

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["grouped_by"] == "priority"
        assert isinstance(response.data["results"], dict)
        # Both seeded issues default to "none" priority.
        assert "none" in response.data["results"]

    @pytest.mark.django_db
    def test_same_group_and_sub_group_is_rejected(self, session_client, workspace, view_issues):
        response = session_client.get(
            _url(workspace.slug) + "?group_by=state_id&sub_group_by=state_id"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
