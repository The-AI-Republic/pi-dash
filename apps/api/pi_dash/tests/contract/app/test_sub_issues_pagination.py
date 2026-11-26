# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the paginated ``SubIssuesEndpoint`` GET.

The endpoint returns the standard pagination envelope with the page under
``sub_issues`` (instead of ``results``) and ``state_distribution`` computed
over the entire child set — the rollup must not change with paging.
"""

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import (
    Issue,
    Project,
    ProjectMember,
    State,
)


@pytest.fixture
def project(db, workspace, create_user):
    project = Project.objects.create(
        name="Sub Issues Project",
        identifier="SUBP",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(
        project=project,
        member=create_user,
        role=20,
        is_active=True,
    )
    return project


@pytest.fixture
def states(db, workspace, project, create_user):
    todo = State.objects.create(
        name="Todo",
        project=project,
        workspace=workspace,
        group="unstarted",
        default=True,
        created_by=create_user,
    )
    started = State.objects.create(
        name="In Progress",
        project=project,
        workspace=workspace,
        group="started",
        created_by=create_user,
    )
    return {"unstarted": todo, "started": started}


def _make_issue(project, user, *, name="issue", state=None, parent=None):
    return Issue.objects.create(
        name=name,
        description_html="<p>body</p>",
        description_stripped="body",
        project=project,
        workspace=project.workspace,
        state=state,
        parent=parent,
        created_by=user,
    )


def _url(slug, project_id, issue_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/sub-issues/"


def _distribution_ids(distribution):
    return {issue_id for ids in distribution.values() for issue_id in ids}


@pytest.mark.contract
class TestSubIssuesPagination:
    @pytest.mark.django_db
    def test_small_parent_fits_one_page(self, session_client, workspace, project, states, create_user):
        parent = _make_issue(project, create_user, name="parent", state=states["unstarted"])
        children = [
            _make_issue(project, create_user, name=f"child {i}", state=states["unstarted"], parent=parent)
            for i in range(3)
        ]

        response = session_client.get(_url(workspace.slug, project.id, parent.id))

        assert response.status_code == http_status.HTTP_200_OK
        data = response.json()
        assert len(data["sub_issues"]) == 3
        assert data["total_count"] == 3
        assert data["next_page_results"] is False
        assert _distribution_ids(data["state_distribution"]) == {str(child.id) for child in children}

    @pytest.mark.django_db
    def test_wide_parent_pages_are_bounded_and_walkable(
        self, session_client, workspace, project, states, create_user
    ):
        parent = _make_issue(project, create_user, name="parent", state=states["unstarted"])
        children = [
            _make_issue(
                project,
                create_user,
                name=f"child {i}",
                state=states["unstarted"] if i % 2 else states["started"],
                parent=parent,
            )
            for i in range(7)
        ]
        child_ids = {str(child.id) for child in children}

        url = _url(workspace.slug, project.id, parent.id)
        response = session_client.get(url, {"per_page": 3})

        assert response.status_code == http_status.HTTP_200_OK
        first_page = response.json()
        assert len(first_page["sub_issues"]) == 3
        assert first_page["total_count"] == 7
        assert first_page["next_page_results"] is True
        # The rollup covers the whole set, not the page
        assert _distribution_ids(first_page["state_distribution"]) == child_ids

        # Walk every page; the union must be the full set with no duplicates
        seen = [issue["id"] for issue in first_page["sub_issues"]]
        page = first_page
        while page["next_page_results"]:
            response = session_client.get(url, {"per_page": 3, "cursor": page["next_cursor"]})
            assert response.status_code == http_status.HTTP_200_OK
            page = response.json()
            assert len(page["sub_issues"]) <= 3
            # state_distribution is identical on every page
            assert page["state_distribution"] == first_page["state_distribution"]
            seen.extend(issue["id"] for issue in page["sub_issues"])

        assert len(seen) == len(set(seen))
        assert set(seen) == child_ids

    @pytest.mark.django_db
    def test_group_by_returns_grouped_page_with_envelope(
        self, session_client, workspace, project, states, create_user
    ):
        parent = _make_issue(project, create_user, name="parent", state=states["unstarted"])
        _make_issue(project, create_user, name="child a", state=states["unstarted"], parent=parent)
        _make_issue(project, create_user, name="child b", state=states["started"], parent=parent)

        response = session_client.get(
            _url(workspace.slug, project.id, parent.id),
            {"group_by": "state_group"},
        )

        assert response.status_code == http_status.HTTP_200_OK
        data = response.json()
        assert isinstance(data["sub_issues"], dict)
        assert set(data["sub_issues"].keys()) == {"unstarted", "started"}
        assert data["total_count"] == 2

    @pytest.mark.django_db
    def test_per_page_above_max_is_rejected(self, session_client, workspace, project, states, create_user):
        parent = _make_issue(project, create_user, name="parent", state=states["unstarted"])

        response = session_client.get(
            _url(workspace.slug, project.id, parent.id),
            {"per_page": 500},
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
