# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The token-auth "wait" endpoint (``pidash issue wait``):

``POST /api/v1/workspaces/<slug>/projects/<project_id>/work-items/<pk>/wait/``

The agent read its open blockers, decided it could not safely proceed, and is
yielding. The call buys back the tick the ending run spent, so waiting costs
no net budget — bounded by one extra pool (PDASHOSS01-204).

Unlike Re-tick and Run AI this endpoint is deliberately **not** guarded
against agent callers: the agent is the intended caller. The contract tested
here is that a refusal is a normal 200 carrying a machine-readable reason,
never an error — the agent is ending its run either way.
"""

from __future__ import annotations

from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone
from rest_framework import status as http_status

from pi_dash.db.models import Issue, IssueActivity, Project, ProjectMember, State
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker
from pi_dash.orchestration import scheduling
from pi_dash.prompting.seed import seed_default_template
from pi_dash.runner.models import AgentRun, AgentRunStatus


@pytest.fixture(autouse=True)
def _on_commit_immediate():
    with mock.patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


@pytest.fixture(autouse=True)
def _no_activity_celery():
    with mock.patch("pi_dash.bgtasks.issue_activities_task.issue_activity.delay"), mock.patch(
        "pi_dash.bgtasks.webhook_task.model_activity.delay"
    ):
        yield


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        project = Project.objects.create(
            name="Wait",
            identifier="WAI",
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
        }


@pytest.fixture
def issue(db, workspace, project, states, create_user):
    seed_default_template()
    with impersonate(create_user):
        i = Issue.objects.create(
            name="Blocked task",
            workspace=workspace,
            project=project,
            state=states["todo"],
            created_by=create_user,
        )
    Issue.all_objects.filter(pk=i.pk).update(state=states["in_progress"])
    i.refresh_from_db()
    IssueAgentTicker.objects.filter(issue=i).delete()
    IssueAgentTicker.objects.create(issue=i, used=4, enabled=True, next_run_at=timezone.now())
    return i


def _wait_url(workspace, issue):
    return f"/api/v1/workspaces/{workspace.slug}/projects/{issue.project_id}/work-items/{issue.id}/wait/"


@pytest.mark.unit
def test_wait_raises_the_cap_and_reports_the_budget(api_key_client, workspace, issue):
    resp = api_key_client.post(_wait_url(workspace, issue), {}, format="json")

    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["applied"] is True
    assert resp.data["reason"] == "waited"
    # The four counters the CLI prints back to the agent.
    assert resp.data["used"] == 4
    assert resp.data["granted"] == 0
    assert resp.data["waited"] == 1
    assert resp.data["cap"] == 11
    assert IssueAgentTicker.objects.get(issue=issue).waited == 1


@pytest.mark.unit
def test_a_second_call_in_the_same_run_buys_another_tick(api_key_client, workspace, issue):
    url = _wait_url(workspace, issue)
    first = api_key_client.post(url, {}, format="json")
    second = api_key_client.post(url, {}, format="json")

    assert [first.data["waited"], second.data["waited"]] == [1, 2]
    assert second.data["cap"] == 12


@pytest.mark.unit
def test_wait_past_the_allowance_is_refused_with_a_machine_readable_reason(api_key_client, workspace, issue):
    IssueAgentTicker.objects.filter(issue=issue).update(waited=10)

    resp = api_key_client.post(_wait_url(workspace, issue), {}, format="json")

    # A refusal is a normal 200 — the agent is yielding either way and must
    # not be derailed by a non-zero exit.
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["applied"] is False
    assert resp.data["reason"] == "wait_cap_reached"
    assert resp.data["waited"] == 10
    assert resp.data["wait_allowance_remaining"] == 0
    assert IssueAgentTicker.objects.get(issue=issue).waited == 10


@pytest.mark.unit
def test_wait_on_an_infinite_pool_is_refused_with_a_reason(api_key_client, workspace, issue, project):
    project.agent_default_max_ticks = -1
    project.save(update_fields=["agent_default_max_ticks"])

    resp = api_key_client.post(_wait_url(workspace, issue), {}, format="json")

    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["applied"] is False
    assert resp.data["reason"] == "infinite_pool"
    assert IssueAgentTicker.objects.get(issue=issue).waited == 0


@pytest.mark.unit
def test_wait_404s_on_an_unknown_work_item(api_key_client, workspace, issue):
    url = f"/api/v1/workspaces/{workspace.slug}/projects/{issue.project_id}/work-items/{issue.workspace_id}/wait/"
    resp = api_key_client.post(url, {}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND


@pytest.mark.unit
def test_wait_from_inside_the_issues_own_run_is_allowed_and_attributed(
    api_key_client, workspace, issue, create_user
):
    """The agent is the intended caller — no ``_refuse_agent_action`` guard.

    Re-tick and Run AI 403 here because they are human levers on the budget;
    a wait adds one tick and the per-issue allowance bounds the total, so the
    run's own id is used for attribution rather than refusal.
    """
    run = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        owner=create_user,
        work_item=issue,
        status=AgentRunStatus.RUNNING,
        phase_kind="coding-task",
        prompt="x",
        started_at=timezone.now(),
    )

    resp = api_key_client.post(
        _wait_url(workspace, issue), {}, format="json", HTTP_X_PI_DASH_RUN_ID=str(run.pk)
    )

    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["applied"] is True
    entry = IssueActivity.objects.get(issue=issue, field=scheduling.WAIT_ACTIVITY_FIELD)
    assert str(run.pk) in entry.comment
