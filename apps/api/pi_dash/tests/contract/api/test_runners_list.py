# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Token-accessible v1 runners list (HA-5, design §14.5).

``GET /api/v1/workspaces/<slug>/runners/`` is the CLI counterpart of the
session-authed web list with the same visibility, filters and serializer,
so ``pidash project runners`` and ``--runner`` name resolution work with
an ``X-Api-Key`` token.
"""

from __future__ import annotations

import pytest
from django.utils import timezone
from rest_framework import status

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import State
from pi_dash.runner.models import Pod, Runner, RunnerStatus


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def state(workspace, project):
    return State.objects.create(
        name="Backlog",
        group="backlog",
        workspace=workspace,
        project=project,
        sequence=1.0,
        default=True,
    )


def _make_runner(user, workspace, pod, name="gpu-box"):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _make_issue(workspace, project, state, user):
    from crum import impersonate

    with impersonate(user):
        return Issue.objects.create(
            name="An issue",
            project=project,
            workspace=workspace,
            state=state,
            created_by=user,
        )


def _path(workspace, **params):
    base = f"/api/v1/workspaces/{workspace.slug}/runners/"
    if params:
        from urllib.parse import urlencode

        return f"{base}?{urlencode(params)}"
    return base


@pytest.mark.unit
def test_list_requires_token(db, workspace, api_client):
    resp = api_client.get(_path(workspace))
    assert resp.status_code in (status.HTTP_401_UNAUTHORIZED, status.HTTP_403_FORBIDDEN)


@pytest.mark.unit
def test_list_returns_visible_runners(db, api_key_client, workspace, project, create_user, pod):
    runner = _make_runner(create_user, workspace, pod)
    resp = api_key_client.get(_path(workspace))
    assert resp.status_code == status.HTTP_200_OK
    assert any(str(row["id"]) == str(runner.id) for row in resp.data)


@pytest.mark.unit
def test_list_project_filter_narrows(db, api_key_client, workspace, project, create_user, pod):
    runner = _make_runner(create_user, workspace, pod)
    resp = api_key_client.get(_path(workspace, project=str(project.id)))
    assert resp.status_code == status.HTTP_200_OK
    assert any(str(row["id"]) == str(runner.id) for row in resp.data)
    # A bogus project narrows the list to nothing instead of leaking runners.
    empty = api_key_client.get(_path(workspace, project="00000000-0000-0000-0000-000000000000"))
    assert empty.status_code == status.HTTP_200_OK
    assert not any(str(row["id"]) == str(runner.id) for row in empty.data)


@pytest.mark.unit
def test_list_assignable_for_adds_row_fields(db, api_key_client, workspace, project, create_user, pod, state):
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user)
    resp = api_key_client.get(_path(workspace, project=str(project.id), assignable_for=str(issue.id)))
    assert resp.status_code == status.HTTP_200_OK
    row = next(r for r in resp.data if str(r["id"]) == str(runner.id))
    assert row["assignable"] is True
    assert row["not_assignable_code"] == ""
    assert row["is_busy"] is False
    assert row["queue_depth"] == 0


@pytest.mark.unit
def test_v1_issue_dispatch_wait_single_item(db, workspace, project, create_user, pod, state):
    """v1 single-item payload carries the waiting object (design §14.3, §14.5)."""
    from pi_dash.api.serializers.issue import IssueSerializer as V1IssueSerializer
    from pi_dash.runner.models import AgentRun, AgentRunStatus, PinKind

    runner = _make_runner(create_user, workspace, pod)
    runner.status = RunnerStatus.OFFLINE
    runner.last_heartbeat_at = None
    runner.save(update_fields=["status", "last_heartbeat_at"])
    issue = _make_issue(workspace, project, state, create_user)
    issue.assigned_pod = pod
    issue.assigned_runner = runner
    issue.save(update_fields=["assigned_pod", "assigned_runner"])
    run = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        status=AgentRunStatus.QUEUED,
        pinned_runner=runner,
        pin_kind=PinKind.DEDICATED,
    )
    assert run.pinned_runner_id == runner.id

    data = V1IssueSerializer(issue).data
    assert data["dispatch_wait"]["reason"] == "runner_offline"
    assert data["dispatch_wait"]["pin_kind"] == "dedicated"
    assert data["dispatch_wait"]["runner"] == str(runner.id)
    assert data["dispatch_wait"]["releasable"] is False


@pytest.mark.unit
def test_v1_issue_dispatch_wait_null_without_waiting_run(db, workspace, project, create_user, pod, state):
    from pi_dash.api.serializers.issue import IssueSerializer as V1IssueSerializer

    issue = _make_issue(workspace, project, state, create_user)
    data = V1IssueSerializer(issue).data
    assert data["dispatch_wait"] is None


@pytest.mark.unit
def test_v1_issue_dispatch_wait_null_on_list_rows(db, workspace, project, create_user, pod, state):
    from pi_dash.api.serializers.issue import IssueSerializer as V1IssueSerializer
    from pi_dash.runner.models import AgentRun, AgentRunStatus, PinKind

    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user)
    AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        status=AgentRunStatus.QUEUED,
        pinned_runner=runner,
        pin_kind=PinKind.DEDICATED,
    )
    rows = V1IssueSerializer([issue], many=True).data
    assert rows[0]["dispatch_wait"] is None


@pytest.mark.unit
def test_list_unknown_workspace_404s(db, api_key_client):
    resp = api_key_client.get("/api/v1/workspaces/no-such-slug-xyz/runners/")
    assert resp.status_code == status.HTTP_404_NOT_FOUND
