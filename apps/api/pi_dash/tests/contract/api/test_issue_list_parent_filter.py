# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the ``parent`` filter on the work-item list endpoint.

``GET .../work-items/?parent=<uuid>`` returns only the children of that parent
so a later agent run can find the child issues an earlier run created before it
splits (PDASHOSS01-169). A malformed ``parent`` is a client error, not a 500.
"""

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import Issue, Project, ProjectMember


@pytest.fixture
def parent_filter_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Parent Filter Project",
        identifier="PFLT",
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


def _make_issue(project, user, *, name, parent=None):
    return Issue.objects.create(
        name=name,
        description_html="<p>body</p>",
        description_stripped="body",
        project=project,
        workspace=project.workspace,
        parent=parent,
        created_by=user,
    )


def _list_url(slug, project_id, query=""):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{query}"


@pytest.mark.contract
class TestWorkItemListParentFilter:
    @pytest.mark.django_db
    def test_parent_filter_returns_only_children(
        self, api_key_client, workspace, parent_filter_project, create_user
    ):
        parent = _make_issue(parent_filter_project, create_user, name="tracking parent")
        child_a = _make_issue(parent_filter_project, create_user, name="child a", parent=parent)
        child_b = _make_issue(parent_filter_project, create_user, name="child b", parent=parent)
        # An unrelated top-level issue that must be excluded from the result.
        _make_issue(parent_filter_project, create_user, name="unrelated")

        url = _list_url(
            workspace.slug, parent_filter_project.id, f"?parent={parent.id}"
        )
        response = api_key_client.get(url)

        assert response.status_code == http_status.HTTP_200_OK
        returned_ids = {str(r["id"]) for r in response.data["results"]}
        assert returned_ids == {str(child_a.id), str(child_b.id)}

    @pytest.mark.django_db
    def test_missing_parent_lists_all(
        self, api_key_client, workspace, parent_filter_project, create_user
    ):
        parent = _make_issue(parent_filter_project, create_user, name="tracking parent")
        _make_issue(parent_filter_project, create_user, name="child", parent=parent)

        url = _list_url(workspace.slug, parent_filter_project.id)
        response = api_key_client.get(url)

        assert response.status_code == http_status.HTTP_200_OK
        # Without a parent filter the parent itself is included.
        returned_ids = {str(r["id"]) for r in response.data["results"]}
        assert str(parent.id) in returned_ids

    @pytest.mark.django_db
    def test_invalid_parent_uuid_is_400_not_500(
        self, api_key_client, workspace, parent_filter_project
    ):
        url = _list_url(workspace.slug, parent_filter_project.id, "?parent=not-a-uuid")
        response = api_key_client.get(url)

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
