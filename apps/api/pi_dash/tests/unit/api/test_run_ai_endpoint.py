# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The token-auth "Run AI" endpoint (``pidash issue run-ai``):

``POST /api/v1/workspaces/<slug>/projects/<project_id>/work-items/<pk>/run-ai/``

Mirrors the web "Run AI" button (``AgentRunListEndpoint._post_run_ai``) but
over an API token, so an operator or an MCP tool can kick a stalled agent from
a terminal. See ``.ai_design`` and PDASHOSS01-166.
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
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod, Runner, RunnerStatus


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with mock.patch("pi_dash.runner.services.pubsub.send_to_runner"):
        yield


@pytest.fixture(autouse=True)
def _on_commit_immediate():
    with mock.patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


@pytest.fixture(autouse=True)
def _no_activity_celery():
    """State moves (e.g. an eligibility bounce) fan activity out over Celery;
    not what these tests exercise."""
    with mock.patch("pi_dash.bgtasks.issue_activities_task.issue_activity.delay"), mock.patch(
        "pi_dash.bgtasks.webhook_task.model_activity.delay"
    ):
        yield


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        project = Project.objects.create(
            name="RunAI",
            identifier="RAI",
            workspace=workspace,
            created_by=create_user,
            agent_default_max_ticks=10,
        )
    # The token caller must be an active project member to pass
    # ProjectEntityPermission on a POST.
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


@pytest.fixture
def states(project, create_user, workspace):
    with impersonate(create_user):
        return {
            "backlog": State.objects.create(
                name="Backlog", project=project, workspace=workspace, group="backlog"
            ),
            "todo": State.objects.create(
                name="Todo", project=project, workspace=workspace, group="unstarted", default=True
            ),
            "in_progress": State.objects.create(
                name="In Progress", project=project, workspace=workspace, group="started"
            ),
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
    # Land in In Progress via a signal-free update so no run or ticker is
    # auto-armed — each test starts from a clean slate and owns its own setup.
    Issue.all_objects.filter(pk=i.pk).update(state=states["in_progress"])
    i.refresh_from_db()
    return i


@pytest.fixture
def online_runner(issue, project, workspace, create_user):
    """An ONLINE runner owned by the issue creator in the issue's pod, so the
    eligibility preflight lets a run dispatch instead of bouncing."""
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=Pod.default_for_project(project),
        name="rai-runner",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _run_ai_url(workspace, issue):
    return f"/api/v1/workspaces/{workspace.slug}/projects/{issue.project_id}/work-items/{issue.id}/run-ai/"


def _make_active_run(issue, create_user):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        work_item=issue,
        status=AgentRunStatus.RUNNING,
        phase_kind="coding-task",
        prompt="x",
        started_at=timezone.now(),
    )


# ---------------------------------------------------------------------------
# Dispatched
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_run_ai_dispatches_a_run(api_key_client, workspace, issue, online_runner):
    resp = api_key_client.post(_run_ai_url(workspace, issue), {}, format="json")
    assert resp.status_code == http_status.HTTP_201_CREATED, resp.data
    assert set(resp.data) == {"id", "status", "executor"}
    run = AgentRun.objects.get(id=resp.data["id"])
    assert run.work_item_id == issue.id
    assert run.trigger == "run_ai"
    assert run.prompt  # rendered from the phase template, not empty


# ---------------------------------------------------------------------------
# Refusal reasons — 409 with a machine-readable ``reason``
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_run_ai_409s_when_a_run_is_active_and_leaves_the_ticker_untouched(
    api_key_client, workspace, issue, online_runner, create_user
):
    _make_active_run(issue, create_user)
    marker = timezone.now()
    IssueAgentTicker.objects.create(issue=issue, used=1, enabled=True, next_run_at=marker)

    resp = api_key_client.post(_run_ai_url(workspace, issue), {}, format="json")

    assert resp.status_code == http_status.HTTP_409_CONFLICT, resp.data
    assert resp.data["reason"] == "active_run_exists"
    # No second run, and the ticker re-time was rolled back.
    assert AgentRun.objects.filter(work_item=issue).count() == 1
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.next_run_at == marker


@pytest.mark.unit
def test_run_ai_409s_no_pod_when_no_pod_is_available(api_key_client, workspace, issue, project):
    # Detach the issue's pod and remove the project's pods so dispatch has
    # nowhere to run (both FKs are PROTECT — detach before delete).
    type(issue).all_objects.filter(pk=issue.pk).update(assigned_pod=None)
    Pod.objects.filter(project=project).delete()

    resp = api_key_client.post(_run_ai_url(workspace, issue), {}, format="json")

    assert resp.status_code == http_status.HTTP_409_CONFLICT, resp.data
    assert resp.data["reason"] == "no_pod"
    assert AgentRun.objects.filter(work_item=issue).count() == 0


@pytest.mark.unit
def test_run_ai_409s_no_eligible_runner_when_no_runner_can_serve(api_key_client, workspace, issue, states):
    # A pod exists but no ONLINE runner owned by an eligible principal, so the
    # eligibility preflight bounces the issue instead of dispatching.
    resp = api_key_client.post(_run_ai_url(workspace, issue), {}, format="json")

    assert resp.status_code == http_status.HTTP_409_CONFLICT, resp.data
    assert resp.data["reason"] == "no_eligible_runner"
    assert AgentRun.objects.filter(work_item=issue).count() == 0


# ---------------------------------------------------------------------------
# Agent self-run guard — X-Pi-Dash-Run-Id
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_agent_cannot_run_ai_its_own_issue(api_key_client, workspace, issue, online_runner, create_user):
    active = _make_active_run(issue, create_user)
    resp = api_key_client.post(
        _run_ai_url(workspace, issue),
        {},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(active.id),
    )
    assert resp.status_code == http_status.HTTP_403_FORBIDDEN
    # Only the pre-existing active run — the guard fired before any dispatch.
    assert AgentRun.objects.filter(work_item=issue).count() == 1


@pytest.mark.unit
def test_run_ai_allows_a_run_id_from_a_different_issue(
    api_key_client, workspace, issue, states, online_runner, create_user
):
    """The CLI sends ``X-Pi-Dash-Run-Id`` on every write for the life of the
    run. A run active on *another* issue is not this issue's agent, so the
    call is allowed through and dispatches normally."""
    other = Issue.objects.create(
        name="Other", workspace=workspace, project=issue.project, state=states["todo"], created_by=create_user
    )
    other_run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, work_item=other,
        status=AgentRunStatus.RUNNING, phase_kind="coding-task", prompt="x", started_at=timezone.now(),
    )
    resp = api_key_client.post(
        _run_ai_url(workspace, issue),
        {},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(other_run.id),
    )
    assert resp.status_code == http_status.HTTP_201_CREATED, resp.data
    assert AgentRun.objects.filter(work_item=issue).count() == 1


# ---------------------------------------------------------------------------
# Permissions
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_run_ai_404s_for_a_missing_issue(api_key_client, workspace, project):
    url = f"/api/v1/workspaces/{workspace.slug}/projects/{project.id}/work-items/{uuid.uuid4()}/run-ai/"
    resp = api_key_client.post(url, {}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND


@pytest.mark.unit
def test_run_ai_refuses_a_caller_without_project_access(api_client, workspace, issue, online_runner):
    """A caller with no access to the project cannot dispatch and learns
    nothing about the issue (no run is created), same as for a missing item."""
    from pi_dash.db.models import User
    from pi_dash.db.models.api import APIToken

    outsider = User.objects.create(email="outsider-rai@example.com", username="outsider_rai")
    token = APIToken.objects.create(user=outsider, label="x", token="outsider-rai-token-1")
    api_client.credentials(HTTP_X_API_KEY=token.token)

    resp = api_client.post(_run_ai_url(workspace, issue), {}, format="json")

    assert resp.status_code in (http_status.HTTP_403_FORBIDDEN, http_status.HTTP_404_NOT_FOUND)
    assert AgentRun.objects.filter(work_item=issue).count() == 0
