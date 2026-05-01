# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Phase 3 cloud HTTP run-lifecycle endpoint tests."""

from __future__ import annotations

import uuid as _uuid

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    RunMessageDedupe,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import tokens


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def enrolled_runner(db, create_user, workspace, pod):
    runner = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="agentR",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
        refresh_token_generation=1,
        enrolled_at=timezone.now(),
    )
    return runner


@pytest.fixture
def runner_token(enrolled_runner):
    token = tokens.mint_access_token(
        runner_id=str(enrolled_runner.id),
        user_id=str(enrolled_runner.owner_id),
        workspace_id=str(enrolled_runner.workspace_id),
        rtg=1,
    )
    return token.raw


@pytest.fixture
def assigned_run(db, create_user, workspace, pod, enrolled_runner):
    return AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        runner=enrolled_runner,
        prompt="x",
        status=AgentRunStatus.ASSIGNED,
        assigned_at=timezone.now(),
    )


@pytest.mark.unit
def test_accept_endpoint_marks_running(
    db, api_client, runner_token, assigned_run
):
    resp = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/accept/",
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 200, resp.data
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.RUNNING


@pytest.mark.unit
def test_run_endpoint_rejects_other_runner(
    db, api_client, runner_token, assigned_run, create_user, workspace, pod
):
    """An access token issued for runner A must not be accepted on a
    run owned by runner B."""
    other_runner = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="agentB",
        refresh_token_generation=1,
    )
    other_run = AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        runner=other_runner,
        prompt="for B",
        status=AgentRunStatus.ASSIGNED,
        assigned_at=timezone.now(),
    )
    resp = api_client.post(
        f"/api/v1/runner/runs/{other_run.id}/accept/",
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 403
    assert resp.data["error"] == "run_not_owned_by_runner"


@pytest.mark.unit
def test_idempotency_key_dedupes_duplicate(
    db, api_client, runner_token, assigned_run
):
    msg_id = _uuid.uuid4().hex
    first = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/started/",
        {"thread_id": "sess_xyz"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
        HTTP_IDEMPOTENCY_KEY=msg_id,
    )
    second = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/started/",
        {"thread_id": "sess_xyz"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
        HTTP_IDEMPOTENCY_KEY=msg_id,
    )
    assert first.status_code == 200
    assert second.status_code == 200
    assert second.data.get("duplicate") is True
    assert RunMessageDedupe.objects.filter(
        run=assigned_run, message_id=msg_id
    ).count() == 1


@pytest.mark.unit
def test_complete_endpoint_marks_terminal_and_drains(
    db, api_client, runner_token, assigned_run
):
    resp = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/complete/",
        {"done_payload": {"summary": "done"}},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 200
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.COMPLETED
    assert assigned_run.ended_at is not None
