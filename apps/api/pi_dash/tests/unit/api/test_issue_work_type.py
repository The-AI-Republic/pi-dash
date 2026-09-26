# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The work-type axis through the external API (PDASHOSS01-234):

- ``work_type`` on ``PATCH .../work-items/<pk>/`` — per-issue override,
  validated against the code-owned registry, and locked to humans once the
  issue is in a ticking stage.
- ``Project.default_work_type`` — resolved at creation and stamped by the
  data migration on pre-existing rows.
- ``AgentRun.work_type`` — stamped at run creation like ``phase_kind``.
"""

from __future__ import annotations

from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone
from rest_framework import status as http_status

from pi_dash.db.models import Issue, Project, ProjectMember, State
from pi_dash.prompting.seed import seed_default_template
from pi_dash.runner.models import AgentRun, AgentRunStatus


@pytest.fixture(autouse=True)
def _no_celery():
    with (
        mock.patch("pi_dash.api.views.issue.issue_activity.delay"),
        mock.patch("pi_dash.api.views.issue.model_activity.delay"),
        mock.patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()),
    ):
        yield


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        project = Project.objects.create(
            name="WorkTyped",
            identifier="WTP",
            workspace=workspace,
            created_by=create_user,
            repo_url="https://example.com/repo.git",
        )
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


@pytest.fixture
def states(project, create_user, workspace):
    with impersonate(create_user):
        return {
            "todo": State.objects.create(
                name="Todo", project=project, workspace=workspace, group="unstarted", default=True
            ),
            "in_progress": State.objects.create(
                name="In Progress", project=project, workspace=workspace, group="started"
            ),
        }


def _make_issue(workspace, project, state, create_user):
    seed_default_template()
    with impersonate(create_user):
        issue = Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=state,
            created_by=create_user,
        )
    return issue


@pytest.fixture
def issue(db, workspace, project, states, create_user):
    i = _make_issue(workspace, project, states["todo"], create_user)
    Issue.all_objects.filter(pk=i.pk).update(state=states["in_progress"])
    i.refresh_from_db()
    return i


def _patch_url(workspace, issue):
    return f"/api/v1/workspaces/{workspace.slug}/projects/{issue.project_id}/work-items/{issue.id}/"


@pytest.mark.unit
def test_project_default_resolves_from_repo_binding(db, workspace, create_user):
    with impersonate(create_user):
        with_repo = Project.objects.create(
            name="Repo", identifier="RP1", workspace=workspace, created_by=create_user, repo_url="https://x/y.git"
        )
        without_repo = Project.objects.create(
            name="NoRepo", identifier="RP2", workspace=workspace, created_by=create_user
        )
    assert with_repo.default_work_type == "software"
    assert without_repo.default_work_type == "general"


@pytest.mark.unit
def test_human_patch_sets_and_clears_work_type(api_key_client, workspace, issue):
    resp = api_key_client.patch(_patch_url(workspace, issue), {"work_type": "general"}, format="json")
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    issue.refresh_from_db()
    assert issue.work_type == "general"
    # "" normalizes back to the inherit state (NULL), not an empty string.
    resp = api_key_client.patch(_patch_url(workspace, issue), {"work_type": ""}, format="json")
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    issue.refresh_from_db()
    assert issue.work_type is None


@pytest.mark.unit
def test_patch_rejects_an_unknown_work_type(api_key_client, workspace, issue):
    resp = api_key_client.patch(_patch_url(workspace, issue), {"work_type": "carpentry"}, format="json")
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
    assert "work_type" in resp.data


@pytest.mark.unit
def test_agent_cannot_change_work_type_while_issue_is_worked(api_key_client, workspace, issue, create_user):
    run = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        work_item=issue,
        status=AgentRunStatus.RUNNING,
        phase_kind="coding-task",
        prompt="x",
        started_at=timezone.now(),
    )
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"work_type": "general"},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(run.id),
    )
    assert resp.status_code == http_status.HTTP_403_FORBIDDEN
    issue.refresh_from_db()
    assert issue.work_type is None
    # Restating the current value is a no-op, not an error.
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"work_type": None, "priority": "high"},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data


@pytest.mark.unit
def test_human_can_change_work_type_while_issue_is_worked(api_key_client, workspace, issue):
    # No run attribution header → a human move; the lock does not apply.
    resp = api_key_client.patch(_patch_url(workspace, issue), {"work_type": "software"}, format="json")
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    issue.refresh_from_db()
    assert issue.work_type == "software"


@pytest.mark.unit
def test_work_type_for_issue_resolves_override_then_project_default(db, workspace, project, states, create_user):
    from pi_dash.orchestration.service import _work_type_for_issue

    issue = _make_issue(workspace, project, states["todo"], create_user)
    # The fixture project is repo-bound → "software" at creation.
    assert _work_type_for_issue(issue) == "software"
    issue.work_type = "general"
    issue.save()
    assert _work_type_for_issue(issue) == "general"
