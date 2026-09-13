# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The run's two write-backs to the ticking clock through the external API:

- ``POST /api/v1/workspaces/<slug>/agent-runs/<run_id>/yield/`` — the outcome
  (``pidash run yield``), design §7.
- ``X-Pi-Dash-Run-Id`` on ``PATCH .../work-items/<pk>/`` — "this state move
  was made from inside an agent run", design §5.6.
"""

from __future__ import annotations

import uuid
from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone
from rest_framework import status as http_status

from pi_dash.db.models import Issue, Project, ProjectMember, State
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker
from pi_dash.prompting.seed import seed_default_template
from pi_dash.runner.models import AgentRun, AgentRunStatus


@pytest.fixture(autouse=True)
def _no_celery():
    """The work-item PATCH fans activity out over Celery; not what these
    tests are about."""
    with mock.patch("pi_dash.api.views.issue.issue_activity.delay"), mock.patch(
        "pi_dash.api.views.issue.model_activity.delay"
    ), mock.patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        project = Project.objects.create(
            name="Yield",
            identifier="YLD",
            workspace=workspace,
            created_by=create_user,
            agent_default_max_ticks=10,
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
            "in_review": State.objects.create(name="In Review", project=project, workspace=workspace, group="review"),
        }


@pytest.fixture
def issue(db, workspace, project, states, create_user):
    seed_default_template()
    with impersonate(create_user):
        i = Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=states["todo"],
            created_by=create_user,
        )
    Issue.all_objects.filter(pk=i.pk).update(state=states["in_progress"])
    i.refresh_from_db()
    return i


@pytest.fixture
def active_run(issue, create_user):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        work_item=issue,
        status=AgentRunStatus.RUNNING,
        phase_kind="coding-task",
        prompt="x",
        started_at=timezone.now(),
    )


def _yield_url(workspace, run_id):
    return f"/api/v1/workspaces/{workspace.slug}/agent-runs/{run_id}/yield/"


def _patch_url(workspace, issue):
    return f"/api/v1/workspaces/{workspace.slug}/projects/{issue.project_id}/work-items/{issue.id}/"


# ---------------------------------------------------------------------------
# run yield
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_yield_writes_the_outcome_on_the_run(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(
        _yield_url(workspace, active_run.id),
        {"outcome": "done", "note": "  approved  "},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(active_run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["outcome"] == "done"
    assert resp.data["work_item_id"] == str(issue.id)
    active_run.refresh_from_db()
    assert active_run.done_payload["status"] == "done"
    assert active_run.done_payload["note"] == "approved"
    assert "yielded_at" in active_run.done_payload


@pytest.mark.unit
def test_yield_accepts_every_vocabulary_word(api_key_client, workspace, issue, active_run):
    for outcome in ("progressed", "waiting_on_human", "waiting_on_external", "done", "blocked"):
        resp = api_key_client.post(_yield_url(workspace, active_run.id), {"outcome": outcome}, format="json")
        assert resp.status_code == http_status.HTTP_200_OK, (outcome, resp.data)
    active_run.refresh_from_db()
    assert active_run.done_payload["status"] == "blocked"


@pytest.mark.unit
def test_yield_rejects_an_unknown_outcome(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(_yield_url(workspace, active_run.id), {"outcome": "finished"}, format="json")
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
    assert "done" in resp.data["allowed"]


@pytest.mark.unit
def test_yield_rejects_a_mismatched_header(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(
        _yield_url(workspace, active_run.id),
        {"outcome": "done"},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(uuid.uuid4()),
    )
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST


@pytest.mark.unit
def test_yield_404s_for_a_stale_or_foreign_run(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(_yield_url(workspace, uuid.uuid4()), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND


@pytest.mark.unit
def test_yield_409s_when_the_run_is_no_longer_active(api_key_client, workspace, issue, active_run):
    AgentRun.objects.filter(pk=active_run.pk).update(status=AgentRunStatus.COMPLETED)
    resp = api_key_client.post(_yield_url(workspace, active_run.id), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_409_CONFLICT


@pytest.mark.unit
def test_yield_requires_workspace_membership(api_client, workspace, issue, active_run):
    from pi_dash.db.models import User
    from pi_dash.db.models.api import APIToken

    outsider = User.objects.create(email="outsider@example.com", username="outsider")
    token = APIToken.objects.create(user=outsider, label="x", token="outsider-token-1")
    api_client.credentials(HTTP_X_API_KEY=token.token)
    resp = api_client.post(_yield_url(workspace, active_run.id), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND


# ---------------------------------------------------------------------------
# PATCH with X-Pi-Dash-Run-Id — agent move vs human move
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_patch_with_run_id_header_is_an_agent_move(api_key_client, workspace, issue, states, active_run):
    """An agent moving the issue from inside its run queues the next stage's
    entry on the clock (it counts); no second run is created while the
    agent's own run is active."""
    IssueAgentTicker.objects.create(issue=issue, used=2, enabled=True, next_run_at=timezone.now())
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"state": str(states["in_review"].id)},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(active_run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    issue.refresh_from_db()
    assert issue.state_id == states["in_review"].id
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.pending_entry is True
    assert ticker.pending_entry_free is False
    assert ticker.used == 2
    assert AgentRun.objects.filter(work_item=issue).count() == 1


@pytest.mark.unit
def test_patch_with_run_id_header_and_spent_pool_parks(api_key_client, workspace, issue, states, active_run):
    IssueAgentTicker.objects.create(issue=issue, used=10, enabled=True, next_run_at=timezone.now())
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"state": str(states["in_review"].id)},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(active_run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    issue.refresh_from_db()
    assert issue.state_id == states["in_review"].id  # the truthful state still lands
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.enabled is False
    assert ticker.pending_entry is False
    assert ticker.disarm_reason == "cap_hit"


@pytest.mark.unit
def test_patch_without_header_is_a_human_move(api_key_client, workspace, issue, states, active_run):
    """Same request, no header: a human move — free, and because a run is
    active it is queued as a free entry."""
    IssueAgentTicker.objects.create(issue=issue, used=10, enabled=False, disarm_reason="cap_hit")
    resp = api_key_client.patch(
        _patch_url(workspace, issue), {"state": str(states["in_review"].id)}, format="json"
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.pending_entry is True
    assert ticker.pending_entry_free is True
    assert ticker.used == 10


@pytest.mark.unit
@pytest.mark.parametrize("header", ["not-a-uuid", str(uuid.uuid4())])
def test_patch_rejects_an_invalid_or_foreign_run_id(api_key_client, workspace, issue, states, active_run, header):
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"state": str(states["in_review"].id)},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=header,
    )
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
    issue.refresh_from_db()
    assert issue.state_id == states["in_progress"].id


@pytest.mark.unit
def test_patch_rejects_a_run_that_is_no_longer_active(api_key_client, workspace, issue, states, active_run):
    AgentRun.objects.filter(pk=active_run.pk).update(status=AgentRunStatus.COMPLETED)
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"state": str(states["in_review"].id)},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(active_run.id),
    )
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
