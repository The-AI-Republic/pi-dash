# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for ``GET .../work-items/<id>/tree/`` (PDASHOSS01-405).

Backs ``pidash issue tree``: a parent plus its children (optionally every
descendant) with a rollup per state group, ``done`` counting both completed
and cancelled children as closed.
"""

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import Issue, IssueAssignee, Project, ProjectMember, State
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod


@pytest.fixture
def tree_project(db, workspace, create_user):
    project = Project.objects.create(name="Tree", identifier="TREE", workspace=workspace, created_by=create_user)
    ProjectMember.objects.get_or_create(project=project, member=create_user, defaults={"role": 20, "is_active": True})
    return project


@pytest.fixture
def states(tree_project):
    groups = {
        "backlog": "Backlog",
        "unstarted": "Todo",
        "started": "In Progress",
        "review": "In Review",
        "completed": "Done",
        "cancelled": "Cancelled",
    }
    return {group: State.objects.create(name=name, project=tree_project, group=group) for group, name in groups.items()}


@pytest.fixture
def make_issue(tree_project, states, create_user):
    def _make(name, *, parent=None, group="unstarted", project=None):
        project = project or tree_project
        return Issue.objects.create(
            name=name,
            project=project,
            workspace=project.workspace,
            state=states[group] if project == tree_project else None,
            parent=parent,
            created_by=create_user,
        )

    return _make


def _ident(issue):
    return f"{issue.project.identifier}-{issue.sequence_id}"


def _url(slug, issue, query=""):
    return f"/api/v1/workspaces/{slug}/projects/{issue.project_id}/work-items/{issue.id}/tree/{query}"


@pytest.mark.contract
@pytest.mark.django_db
class TestIssueTree:
    def test_rollup_counts_mixed_state_groups_cancelled_is_closed(self, api_key_client, workspace, make_issue):
        epic = make_issue("Epic", group="started")
        make_issue("backlog child", parent=epic, group="backlog")
        make_issue("todo child", parent=epic, group="unstarted")
        make_issue("wip child", parent=epic, group="started")
        make_issue("done child", parent=epic, group="completed")
        make_issue("done child 2", parent=epic, group="completed")
        make_issue("cancelled child", parent=epic, group="cancelled")
        make_issue("unrelated")

        resp = api_key_client.get(_url(workspace.slug, epic))

        assert resp.status_code == http_status.HTTP_200_OK, resp.content
        body = resp.json()
        assert body["issue"]["identifier"] == _ident(epic)
        assert body["issue"]["state_group"] == "started"
        assert body["recursive"] is False
        assert body["truncated"] is False
        assert body["rollup"] == {
            "total": 6,
            "done": 3,
            "open": 3,
            "by_state_group": {
                "backlog": 1,
                "unstarted": 1,
                "started": 1,
                "review": 0,
                "test": 0,
                "completed": 2,
                "cancelled": 1,
            },
        }
        assert [c["name"] for c in body["children"]] == [
            "backlog child",
            "todo child",
            "wip child",
            "done child",
            "done child 2",
            "cancelled child",
        ]
        assert all(c["depth"] == 1 and c["parent"] == _ident(epic) for c in body["children"])

    def test_child_carries_state_assignees_and_active_run(
        self, api_key_client, workspace, make_issue, create_user, tree_project
    ):
        epic = make_issue("Epic")
        wip = make_issue("wip", parent=epic, group="started")
        IssueAssignee.objects.create(
            issue=wip, assignee=create_user, project=tree_project, workspace=tree_project.workspace
        )
        pod = Pod.default_for_project(tree_project)
        AgentRun.objects.create(
            workspace=workspace,
            created_by=create_user,
            pod=pod,
            work_item=wip,
            status=AgentRunStatus.COMPLETED,
        )
        run = AgentRun.objects.create(
            workspace=workspace,
            created_by=create_user,
            pod=pod,
            work_item=wip,
            status=AgentRunStatus.RUNNING,
        )
        idle = make_issue("idle", parent=epic)

        body = api_key_client.get(_url(workspace.slug, epic)).json()

        by_name = {c["name"]: c for c in body["children"]}
        assert by_name["wip"]["identifier"] == _ident(wip)
        assert by_name["wip"]["state"] == "In Progress"
        assert by_name["wip"]["state_group"] == "started"
        assert by_name["wip"]["assignees"] == [{"id": str(create_user.id), "display_name": create_user.display_name}]
        assert by_name["wip"]["active_run"] == {"id": str(run.id), "status": "running"}
        assert by_name["idle"]["identifier"] == _ident(idle)
        assert by_name["idle"]["assignees"] == []
        assert by_name["idle"]["active_run"] is None

    def test_recursive_walks_descendants_depth_first(self, api_key_client, workspace, make_issue):
        epic = make_issue("Epic")
        a = make_issue("a", parent=epic, group="completed")
        b = make_issue("b", parent=epic)
        a1 = make_issue("a1", parent=a, group="cancelled")
        make_issue("a1x", parent=a1, group="started")

        flat = api_key_client.get(_url(workspace.slug, epic)).json()
        assert [c["name"] for c in flat["children"]] == ["a", "b"]
        assert flat["rollup"]["total"] == 2

        deep = api_key_client.get(_url(workspace.slug, epic, "?recursive=true")).json()
        assert deep["recursive"] is True
        assert [(c["name"], c["depth"], c["parent"]) for c in deep["children"]] == [
            ("a", 1, _ident(epic)),
            ("a1", 2, _ident(a)),
            ("a1x", 3, _ident(a1)),
            ("b", 1, _ident(epic)),
        ]
        assert deep["rollup"]["total"] == 4
        assert deep["rollup"]["done"] == 2
        assert deep["rollup"]["open"] == 2
        assert _ident(b) in [c["identifier"] for c in deep["children"]]

    def test_hidden_and_archived_children_are_left_out(self, api_key_client, workspace, make_issue, create_user):
        from django.utils import timezone

        epic = make_issue("Epic")
        make_issue("visible", parent=epic)
        archived = make_issue("archived", parent=epic)
        archived.archived_at = timezone.now()
        archived.save()
        other = Project.objects.create(name="Other", identifier="OTH", workspace=workspace, created_by=create_user)
        ProjectMember.objects.filter(project=other, member=create_user).update(is_active=False)
        make_issue("not a member", parent=epic, project=other)

        body = api_key_client.get(_url(workspace.slug, epic)).json()

        assert [c["name"] for c in body["children"]] == ["visible"]
        assert body["rollup"]["total"] == 1

    def test_no_children_is_an_empty_rollup(self, api_key_client, workspace, make_issue):
        leaf = make_issue("leaf")
        body = api_key_client.get(_url(workspace.slug, leaf)).json()
        assert body["children"] == []
        assert body["rollup"]["total"] == 0
        assert body["rollup"]["done"] == 0
        assert body["rollup"]["open"] == 0

    def test_bad_recursive_value_is_400(self, api_key_client, workspace, make_issue):
        leaf = make_issue("leaf")
        resp = api_key_client.get(_url(workspace.slug, leaf, "?recursive=maybe"))
        assert resp.status_code == http_status.HTTP_400_BAD_REQUEST

    def test_unknown_issue_is_404(self, api_key_client, workspace, tree_project):
        url = (
            f"/api/v1/workspaces/{workspace.slug}/projects/{tree_project.id}/work-items/"
            "00000000-0000-0000-0000-000000000000/tree/"
        )
        assert api_key_client.get(url).status_code == http_status.HTTP_404_NOT_FOUND
