# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the agent-facing relation endpoints (PDASHOSS01-199).

``.../work-items/<id>/relations/{grouped,relate,unrelate}/`` back
``pidash issue relations|relate|unrelate``; the single-item work-item reads
back the ``relations`` block ``pidash issue get`` prints.
"""

from unittest import mock

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import Issue, IssueRelation, Project, ProjectMember, State


@pytest.fixture(autouse=True)
def _no_activity():
    with mock.patch("pi_dash.bgtasks.issue_activities_task.issue_activity.delay"):
        yield


@pytest.fixture
def rel_project(db, workspace, create_user):
    project = Project.objects.create(name="Relations", identifier="REL", workspace=workspace, created_by=create_user)
    ProjectMember.objects.get_or_create(project=project, member=create_user, defaults={"role": 20, "is_active": True})
    return project


@pytest.fixture
def todo(rel_project):
    return State.objects.create(name="Todo", project=rel_project, group="unstarted")


@pytest.fixture
def make_issue(rel_project, todo, create_user):
    def _make(name, project=None, state=None):
        project = project or rel_project
        return Issue.objects.create(
            name=name,
            project=project,
            workspace=project.workspace,
            state=state or todo,
            created_by=create_user,
        )

    return _make


def _ident(issue):
    return f"{issue.project.identifier}-{issue.sequence_id}"


def _base(slug, issue):
    return f"/api/v1/workspaces/{slug}/projects/{issue.project_id}/work-items/{issue.id}/relations"


@pytest.mark.contract
@pytest.mark.django_db
class TestRelateRoundTrip:
    def test_relate_list_unrelate(self, api_key_client, workspace, make_issue):
        a, b, c = make_issue("Handler"), make_issue("Model"), make_issue("Query")
        base = _base(workspace.slug, a)

        resp = api_key_client.post(
            f"{base}/relate/",
            {"relation_type": "blocked_by", "issues": [_ident(b), str(c.id)]},
            format="json",
        )
        assert resp.status_code == http_status.HTTP_200_OK, resp.content
        body = resp.json()
        assert body["issue"] == _ident(a)
        assert body["created"] == [_ident(b), _ident(c)]
        assert body["unchanged"] == [] and body["conflicts"] == []
        assert [i["identifier"] for i in body["relations"]["blocked_by"]] == [_ident(b), _ident(c)]
        assert body["relations"]["blocked_by"][0]["name"] == "Model"
        assert body["relations"]["blocked_by"][0]["state"] == "Todo"

        # Idempotent re-relate: 200, nothing created.
        again = api_key_client.post(
            f"{base}/relate/", {"relation_type": "blocked_by", "issues": [_ident(b)]}, format="json"
        )
        assert again.status_code == http_status.HTTP_200_OK
        assert again.json()["created"] == [] and again.json()["unchanged"] == [_ident(b)]
        assert IssueRelation.objects.filter(issue=a).count() == 2

        listed = api_key_client.get(f"{base}/grouped/")
        assert listed.status_code == http_status.HTTP_200_OK
        assert [i["identifier"] for i in listed.json()["relations"]["blocked_by"]] == [_ident(b), _ident(c)]
        # From the blocker's side it reads as blocking.
        from_b = api_key_client.get(f"{_base(workspace.slug, b)}/grouped/").json()
        assert [i["identifier"] for i in from_b["relations"]["blocking"]] == [_ident(a)]

        removed = api_key_client.post(
            f"{base}/unrelate/", {"relation_type": "blocked_by", "issues": [_ident(b)]}, format="json"
        )
        assert removed.status_code == http_status.HTTP_200_OK
        assert removed.json()["removed"] == [_ident(b)]
        assert [i["identifier"] for i in removed.json()["relations"]["blocked_by"]] == [_ident(c)]

        noop = api_key_client.post(
            f"{base}/unrelate/", {"relation_type": "blocked_by", "issues": [_ident(b)]}, format="json"
        )
        assert noop.status_code == http_status.HTTP_200_OK
        assert noop.json()["removed"] == [] and noop.json()["not_related"] == [_ident(b)]

    def test_issue_get_carries_relations_block(self, api_key_client, workspace, make_issue):
        a, b = make_issue("Handler"), make_issue("Model")
        api_key_client.post(
            f"{_base(workspace.slug, a)}/relate/",
            {"relation_type": "blocked_by", "issues": [_ident(b)]},
            format="json",
        )
        by_identifier = api_key_client.get(f"/api/v1/workspaces/{workspace.slug}/work-items/{_ident(a)}/")
        assert by_identifier.status_code == http_status.HTTP_200_OK
        rel = by_identifier.json()["relations"]
        assert [i["identifier"] for i in rel["blocked_by"]] == [_ident(b)]
        assert rel["blocked_by"][0]["name"] == "Model"
        # PDASHOSS01-197's summary rides alongside, unchanged.
        assert by_identifier.json()["has_open_blockers"] is True

        detail = api_key_client.get(f"/api/v1/workspaces/{workspace.slug}/projects/{a.project_id}/work-items/{a.id}/")
        assert [i["identifier"] for i in detail.json()["relations"]["blocked_by"]] == [_ident(b)]

    def test_list_endpoint_payload_has_no_relations_block(self, api_key_client, workspace, make_issue):
        a = make_issue("Handler")
        resp = api_key_client.get(f"/api/v1/workspaces/{workspace.slug}/projects/{a.project_id}/work-items/")
        assert resp.status_code == http_status.HTTP_200_OK
        assert all("relations" not in row for row in resp.json()["results"])


@pytest.mark.contract
@pytest.mark.django_db
class TestRelateValidation:
    def test_target_in_a_project_the_caller_cannot_see_is_refused(
        self, api_key_client, workspace, make_issue, create_user
    ):
        secret = Project.objects.create(name="Secret", identifier="SEC", workspace=workspace, created_by=create_user)
        ProjectMember.objects.filter(project=secret, member=create_user).delete()
        hidden = make_issue("Hidden", project=secret, state=State.objects.create(name="Todo", project=secret))
        a = make_issue("Handler")

        resp = api_key_client.post(
            f"{_base(workspace.slug, a)}/relate/",
            {"relation_type": "blocked_by", "issues": [_ident(hidden)]},
            format="json",
        )
        assert resp.status_code == http_status.HTTP_404_NOT_FOUND
        assert resp.json()["unresolved"] == [_ident(hidden)]
        assert not IssueRelation.objects.exists()

    def test_one_bad_target_writes_nothing(self, api_key_client, workspace, make_issue):
        a, b = make_issue("Handler"), make_issue("Model")
        resp = api_key_client.post(
            f"{_base(workspace.slug, a)}/relate/",
            {"relation_type": "blocked_by", "issues": [_ident(b), "REL-99999"]},
            format="json",
        )
        assert resp.status_code == http_status.HTTP_404_NOT_FOUND
        assert resp.json()["unresolved"] == ["REL-99999"]
        assert not IssueRelation.objects.exists()

    @pytest.mark.parametrize(
        "payload",
        [
            {"relation_type": "depends_on", "issues": ["REL-1"]},
            {"relation_type": "blocked_by", "issues": []},
            {"relation_type": "blocked_by"},
        ],
    )
    def test_bad_request(self, api_key_client, workspace, make_issue, payload):
        a = make_issue("Handler")
        resp = api_key_client.post(f"{_base(workspace.slug, a)}/relate/", payload, format="json")
        assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "error" in resp.json()

    def test_self_relation_is_a_400(self, api_key_client, workspace, make_issue):
        a = make_issue("Handler")
        resp = api_key_client.post(
            f"{_base(workspace.slug, a)}/relate/",
            {"relation_type": "blocked_by", "issues": [_ident(a)]},
            format="json",
        )
        assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
        assert not IssueRelation.objects.exists()

    def test_conflicting_relation_is_reported_not_overwritten(self, api_key_client, workspace, make_issue):
        a, b = make_issue("Handler"), make_issue("Model")
        base = _base(workspace.slug, a)
        api_key_client.post(f"{base}/relate/", {"relation_type": "relates_to", "issues": [_ident(b)]}, format="json")
        resp = api_key_client.post(
            f"{base}/relate/", {"relation_type": "blocked_by", "issues": [_ident(b)]}, format="json"
        )
        assert resp.status_code == http_status.HTTP_200_OK
        assert resp.json()["conflicts"] == [{"identifier": _ident(b), "existing_relation": "relates_to"}]
        assert IssueRelation.objects.get().relation_type == "relates_to"
