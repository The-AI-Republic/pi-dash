# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for filters and field selection on the v1 work-item list
(``GET /api/v1/workspaces/{slug}/projects/{project_id}/work-items/``).
"""

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest
from django.conf import settings
from django.utils import timezone
from rest_framework import status

from pi_dash.db.models import Issue, IssueLabel, Label, Project, ProjectMember, State


@pytest.fixture
def proj(db, workspace, create_user):
    project = Project.objects.create(
        name="Filter Project", identifier="FLT", workspace=workspace, created_by=create_user
    )
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


def _state(project, user, name, group, **kw):
    return State.objects.create(
        name=name, project=project, workspace=project.workspace, group=group, created_by=user, **kw
    )


def _issue(project, user, name, *, state, priority="none", parent=None):
    return Issue.objects.create(
        name=name,
        project=project,
        workspace=project.workspace,
        state=state,
        priority=priority,
        parent=parent,
        created_by=user,
    )


def _label(project, user, name):
    return Label.objects.create(name=name, project=project, workspace=project.workspace, created_by=user)


def _attach(issue, label, user):
    return IssueLabel.objects.create(
        issue=issue, label=label, project=issue.project, workspace=issue.workspace, created_by=user
    )


@pytest.fixture
def world(proj, create_user):
    u = create_user
    backlog = _state(proj, u, "Backlog", "backlog", default=True)
    todo = _state(proj, u, "Todo", "unstarted")
    done = _state(proj, u, "Done", "completed")
    epic = _issue(proj, u, "Epic", state=todo, priority="high")
    child_backlog = _issue(proj, u, "Child backlog", state=backlog, priority="urgent", parent=epic)
    child_done = _issue(proj, u, "Child done", state=done, priority="low", parent=epic)
    loose = _issue(proj, u, "Loose backlog", state=backlog, priority="high")
    bug = _label(proj, u, "Bug")
    _attach(child_backlog, bug, u)
    _attach(loose, bug, u)
    return {
        "backlog": backlog,
        "todo": todo,
        "done": done,
        "epic": epic,
        "child_backlog": child_backlog,
        "child_done": child_done,
        "loose": loose,
        "bug": bug,
    }


def _url(workspace, project):
    return f"/api/v1/workspaces/{workspace.slug}/projects/{project.id}/work-items/"


def _names(response):
    assert response.status_code == status.HTTP_200_OK, response.content
    return {r["name"] for r in response.data["results"]}


ALL = {"Epic", "Child backlog", "Child done", "Loose backlog"}


@pytest.mark.contract
@pytest.mark.django_db
class TestWorkItemListFilters:
    def test_no_filters_returns_everything(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj))
        assert _names(response) == ALL
        assert response.data["total_count"] == 4

    def test_filter_by_state_id(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"state": str(world["backlog"].id)})
        assert _names(response) == {"Child backlog", "Loose backlog"}

    def test_filter_by_state_name_is_case_insensitive(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"state": "backlog,DONE"})
        assert _names(response) == {"Child backlog", "Loose backlog", "Child done"}

    def test_filter_by_mixed_state_name_and_id(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"state": f"Todo,{world['done'].id}"})
        assert _names(response) == {"Epic", "Child done"}

    def test_unknown_state_name_is_400_listing_valid_names(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"state": "Nope"})
        assert response.status_code == status.HTTP_400_BAD_REQUEST
        error = response.data["error"]
        assert "Nope" in error
        for name in ("Backlog", "Todo", "Done"):
            assert name in error

    def test_filter_by_state_group(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"state_group": "backlog,completed"})
        assert _names(response) == {"Child backlog", "Loose backlog", "Child done"}

    def test_unknown_state_group_is_400(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"state_group": "doing"})
        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert "doing" in response.data["error"]

    def test_filter_by_parent_uuid(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"parent": str(world["epic"].id)})
        assert _names(response) == {"Child backlog", "Child done"}

    def test_filter_by_parent_identifier(self, api_key_client, workspace, proj, world):
        ident = f"flt-{world['epic'].sequence_id}"  # identifier match is case-insensitive
        response = api_key_client.get(_url(workspace, proj), {"parent": ident})
        assert _names(response) == {"Child backlog", "Child done"}

    def test_filter_parent_null_returns_top_level_only(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"parent": "null"})
        assert _names(response) == {"Epic", "Loose backlog"}

    def test_unknown_parent_identifier_is_400(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"parent": "FLT-999"})
        assert response.status_code == status.HTTP_400_BAD_REQUEST

    def test_filter_by_label_name_and_id(self, api_key_client, workspace, proj, world):
        assert _names(api_key_client.get(_url(workspace, proj), {"labels": "bug"})) == {
            "Child backlog",
            "Loose backlog",
        }
        assert _names(api_key_client.get(_url(workspace, proj), {"labels": str(world["bug"].id)})) == {
            "Child backlog",
            "Loose backlog",
        }

    def test_soft_deleted_label_link_does_not_match(self, api_key_client, workspace, proj, world):
        IssueLabel.objects.filter(issue=world["loose"]).update(deleted_at=timezone.now())
        response = api_key_client.get(_url(workspace, proj), {"labels": "Bug"})
        assert _names(response) == {"Child backlog"}

    def test_unknown_label_name_is_400(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"labels": "feature"})
        assert response.status_code == status.HTTP_400_BAD_REQUEST
        assert "Bug" in response.data["error"]

    def test_filter_by_priority(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"priority": "high,urgent"})
        assert _names(response) == {"Epic", "Child backlog", "Loose backlog"}

    def test_unknown_priority_is_400(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(_url(workspace, proj), {"priority": "p0"})
        assert response.status_code == status.HTTP_400_BAD_REQUEST

    def test_combined_filters_and_together(self, api_key_client, workspace, proj, world):
        params = {"state": "Backlog", "parent": f"FLT-{world['epic'].sequence_id}", "priority": "urgent"}
        response = api_key_client.get(_url(workspace, proj), params)
        assert _names(response) == {"Child backlog"}
        assert response.data["total_count"] == 1

    def test_fields_narrows_the_payload(self, api_key_client, workspace, proj, world):
        response = api_key_client.get(
            _url(workspace, proj), {"state": "Backlog", "fields": "id,sequence_id,name,state,parent"}
        )
        assert response.status_code == status.HTTP_200_OK
        for row in response.data["results"]:
            assert set(row) == {"id", "sequence_id", "name", "state", "parent"}

    def test_pagination_and_order_by_with_filters(self, api_key_client, workspace, proj, world):
        url = _url(workspace, proj)
        first = api_key_client.get(url, {"state_group": "backlog,completed", "order_by": "name", "per_page": 2})
        assert first.status_code == status.HTTP_200_OK
        assert [r["name"] for r in first.data["results"]] == ["Child backlog", "Child done"]
        assert first.data["total_count"] == 3
        assert first.data["next_page_results"] is True

        params = {"state_group": "backlog,completed", "order_by": "name", "per_page": 2}
        second = api_key_client.get(url, {**params, "cursor": first.data["next_cursor"]})
        assert [r["name"] for r in second.data["results"]] == ["Loose backlog"]
        assert second.data["next_page_results"] is False

    def test_project_slug_route_applies_filters(self, api_key_client, workspace, proj, world):
        url = f"/api/v1/workspaces/{workspace.slug}/projects/{proj.identifier}/work-items/"
        assert _names(api_key_client.get(url, {"state": "Done"})) == {"Child done"}


@pytest.mark.contract
def test_openapi_schema_lists_the_filter_parameters(tmp_path):
    """The generated OpenAPI document advertises every list filter.

    drf-spectacular binds its schema class at import time and is only enabled
    via ``ENABLE_DRF_SPECTACULAR``, so generate the document in a subprocess.
    """
    out = tmp_path / "schema.json"
    subprocess.run(
        [sys.executable, "manage.py", "spectacular", "--format", "openapi-json", "--file", str(out)],
        cwd=Path(settings.BASE_DIR).parent,
        env={**os.environ, "ENABLE_DRF_SPECTACULAR": "1", "DJANGO_SETTINGS_MODULE": "pi_dash.settings.test"},
        check=True,
        capture_output=True,
    )
    schema = json.loads(out.read_text())
    list_ops = [
        ops["get"]
        for path, ops in schema["paths"].items()
        if path.startswith("/api/v1/") and ops.get("get", {}).get("operationId") == "list_work_items"
    ]
    assert list_ops
    names = {p["name"] for p in list_ops[0]["parameters"] if p["in"] == "query"}
    assert {"state", "state_group", "parent", "labels", "priority", "assignees", "fields", "expand"} <= names
