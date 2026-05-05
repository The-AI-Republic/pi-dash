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


@pytest.mark.unit
def test_fail_endpoint_resume_unavailable_requeues_instead_of_terminating(
    db, api_client, runner_token, assigned_run
):
    """Regression guard: a RunFailed{reason: resume_unavailable} must
    re-queue the run, not stamp it FAILED. Cloud-side recovery for runs
    that miss their pinned session on disk lives here.
    """
    resp = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/fail/",
        {"reason": "resume_unavailable", "detail": "session gone"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 200, resp.data
    assert resp.data.get("rescheduled") is True
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.QUEUED
    assert assigned_run.runner_id is None
    assert assigned_run.pinned_runner_id is None


@pytest.mark.unit
def test_paused_endpoint_posts_question_to_issue_thread(
    db, api_client, runner_token, enrolled_runner, workspace, pod
):
    """Regression guard: RunPaused with a question_for_human must
    surface to the issue's comment thread. Without this, comment-and-run
    flows lose the agent's pause-question entirely.
    """
    from crum import impersonate

    from pi_dash.db.models.issue import Issue, IssueComment
    from pi_dash.db.models.project import Project
    from pi_dash.db.models.state import State

    with impersonate(enrolled_runner.owner):
        project = Project.objects.create(
            name="P",
            identifier="P",
            workspace=workspace,
            created_by=enrolled_runner.owner,
        )
        state = State.objects.create(
            name="In Progress", project=project, group="started"
        )
        issue = Issue.objects.create(
            name="task",
            workspace=workspace,
            project=project,
            state=state,
            created_by=enrolled_runner.owner,
        )
    paused_run = AgentRun.objects.create(
        owner=enrolled_runner.owner,
        created_by=enrolled_runner.owner,
        workspace=workspace,
        pod=pod,
        runner=enrolled_runner,
        work_item=issue,
        prompt="x",
        status=AgentRunStatus.RUNNING,
        assigned_at=timezone.now(),
    )

    from unittest.mock import patch

    # Run on_commit callbacks inline so the post-commit drain helpers
    # don't hold up the assertion.
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        resp = api_client.post(
            f"/api/v1/runner/runs/{paused_run.id}/pause/",
            {
                "payload": {
                    "autonomy": {"question_for_human": "what now?"},
                    "summary": "made progress",
                }
            },
            format="json",
            HTTP_AUTHORIZATION=f"Bearer {runner_token}",
        )

    assert resp.status_code == 200, resp.data
    paused_run.refresh_from_db()
    assert paused_run.status == AgentRunStatus.PAUSED_AWAITING_INPUT
    comments = IssueComment.objects.filter(issue=issue)
    assert comments.exists(), "pause endpoint must post a comment for question_for_human"
    body = comments.first().comment_html
    assert "what now?" in body
    assert "made progress" in body
