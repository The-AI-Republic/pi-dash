# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Phase 3 cloud HTTP run-lifecycle endpoint tests."""

from __future__ import annotations

import uuid as _uuid
from unittest.mock import patch

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
def test_session_open_waits_for_first_poll_before_dispatch(
    db, api_client, runner_token, enrolled_runner, create_user, workspace, pod
):
    enrolled_runner.status = RunnerStatus.OFFLINE
    enrolled_runner.last_heartbeat_at = None
    enrolled_runner.save(update_fields=["status", "last_heartbeat_at"])
    queued = AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        pinned_runner=enrolled_runner,
        prompt="queued",
        status=AgentRunStatus.QUEUED,
    )

    open_resp = api_client.post(
        f"/api/v1/runner/runners/{enrolled_runner.id}/sessions/",
        {
            "version": "test",
            "os": "linux",
            "arch": "x86_64",
            "status": "online",
            "in_flight_run": None,
        },
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )

    assert open_resp.status_code == 201, open_resp.data
    queued.refresh_from_db()
    enrolled_runner.refresh_from_db()
    assert queued.status == AgentRunStatus.QUEUED
    assert queued.runner_id is None
    assert enrolled_runner.status == RunnerStatus.OFFLINE

    sid = open_resp.data["session_id"]
    with (
        patch("pi_dash.runner.views.sessions.outbox.is_pel_drained", return_value=False),
        patch("pi_dash.runner.views.sessions.outbox.read_for_session", return_value=[]),
        patch("pi_dash.runner.views.sessions.outbox.mark_pel_drained"),
        patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()),
    ):
        poll_resp = api_client.post(
            f"/api/v1/runner/runners/{enrolled_runner.id}/sessions/{sid}/poll",
            {
                "ack": [],
                "status": {
                    "status": "online",
                    "in_flight_run": None,
                    "ts": timezone.now().isoformat(),
                },
            },
            format="json",
            HTTP_AUTHORIZATION=f"Bearer {runner_token}",
        )

    assert poll_resp.status_code == 200, poll_resp.data
    queued.refresh_from_db()
    enrolled_runner.refresh_from_db()
    assert queued.status == AgentRunStatus.ASSIGNED
    assert queued.runner_id == enrolled_runner.id
    assert queued.assigned_at is not None
    assert enrolled_runner.status == RunnerStatus.ONLINE


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
def test_complete_endpoint_clears_stale_error(
    db, api_client, runner_token, assigned_run
):
    assigned_run.error = "daemon shutdown requested"
    assigned_run.save(update_fields=["error"])

    resp = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/complete/",
        {"done_payload": {"summary": "done"}},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )

    assert resp.status_code == 200, resp.data
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.COMPLETED
    assert assigned_run.error == ""


@pytest.mark.unit
def test_late_complete_does_not_overwrite_failed_run(
    db, api_client, runner_token, assigned_run
):
    failed = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/fail/",
        {"detail": "reaped by heartbeat"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
        HTTP_IDEMPOTENCY_KEY=_uuid.uuid4().hex,
    )
    assert failed.status_code == 200, failed.data
    assigned_run.refresh_from_db()
    ended_at = assigned_run.ended_at

    completed = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/complete/",
        {"done_payload": {"summary": "late success"}},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
        HTTP_IDEMPOTENCY_KEY=_uuid.uuid4().hex,
    )

    assert completed.status_code == 200, completed.data
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.FAILED
    assert assigned_run.error == "reaped by heartbeat"
    assert assigned_run.ended_at == ended_at
    assert assigned_run.done_payload is None


@pytest.mark.unit
def test_late_started_does_not_revive_failed_run(
    db, api_client, runner_token, assigned_run
):
    assigned_run.status = AgentRunStatus.FAILED
    assigned_run.ended_at = timezone.now()
    assigned_run.error = "reaped by heartbeat"
    assigned_run.save(update_fields=["status", "ended_at", "error"])

    resp = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/started/",
        {"thread_id": "late_thread"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )

    assert resp.status_code == 200, resp.data
    assert resp.data.get("terminal") is True
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.FAILED
    assert assigned_run.thread_id == ""
    assert assigned_run.started_at is None


@pytest.mark.unit
def test_late_resume_unavailable_does_not_requeue_failed_run(
    db, api_client, runner_token, assigned_run, enrolled_runner
):
    assigned_run.status = AgentRunStatus.FAILED
    assigned_run.ended_at = timezone.now()
    assigned_run.error = "reaped by heartbeat"
    assigned_run.save(update_fields=["status", "ended_at", "error"])

    resp = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/fail/",
        {"reason": "resume_unavailable", "detail": "session gone"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )

    assert resp.status_code == 200, resp.data
    assert resp.data.get("terminal") is True
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.FAILED
    assert assigned_run.runner_id == enrolled_runner.id
    assert assigned_run.error == "reaped by heartbeat"


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
def test_fail_endpoint_refusal_records_refused_and_category(
    db, api_client, runner_token, assigned_run
):
    """A RunFailed{reason: refusal} is recorded as terminal REFUSED with the
    safety-classifier category, not a generic FAILED. This is how a Claude
    Fable 5 cyber/bio decline stays queryable apart from a crash.
    """
    resp = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/fail/",
        {
            "reason": "refusal",
            "category": "cyber",
            "detail": "declined under cyber policy",
            "model": "claude-fable-5",
        },
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 200, resp.data
    assert resp.data.get("refused") is True
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.REFUSED
    assert assigned_run.refusal_category == "cyber"
    assert assigned_run.error == "declined under cyber policy"
    assert assigned_run.llm_model == "claude-fable-5"
    assert assigned_run.ended_at is not None


@pytest.mark.unit
def test_fail_endpoint_refusal_unknown_category_normalizes(
    db, api_client, runner_token, assigned_run
):
    """A refusal with a missing/unrecognized category is still recorded as
    REFUSED, with the category normalized to ``unknown`` so the column is
    always populated for a decline."""
    resp = api_client.post(
        f"/api/v1/runner/runs/{assigned_run.id}/fail/",
        {"reason": "refusal", "category": "not_a_real_category"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 200, resp.data
    assert resp.data.get("refused") is True
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.REFUSED
    assert assigned_run.refusal_category == "unknown"


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
