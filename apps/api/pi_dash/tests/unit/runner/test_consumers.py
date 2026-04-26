# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for ``RunnerConsumer``'s sync helpers.

The consumer extends ``AsyncJsonWebsocketConsumer`` but most of the
state-machine logic is delegated to plain sync methods that we can call
directly without spinning up Channels. These tests cover:

- ``_handle_run_paused`` — pause path, including HTML escaping of
  agent-supplied question/summary that gets stored as an IssueComment.
- ``_handle_resume_unavailable`` — re-queue + pin-drop on the typed
  resume-failure path.
"""

from __future__ import annotations

from unittest.mock import patch

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.db.models.issue import Issue, IssueComment
from pi_dash.db.models.project import Project
from pi_dash.db.models.state import State
from pi_dash.runner.consumers import RunnerConsumer
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)


@pytest.fixture
def pod(workspace):
    return Pod.default_for_workspace(workspace)


@pytest.fixture
def online_runner(db, create_user, workspace, pod):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="rX",
        credential_hash="h",
        credential_fingerprint="f" * 12,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture
def issue_in_progress(db, workspace, create_user):
    with impersonate(create_user):
        project = Project.objects.create(
            name="P", identifier="P", workspace=workspace, created_by=create_user
        )
        in_progress = State.objects.create(
            name="In Progress", project=project, group="started"
        )
        return Issue.objects.create(
            name="task",
            workspace=workspace,
            project=project,
            state=in_progress,
            created_by=create_user,
        )


@pytest.fixture(autouse=True)
def _on_commit_immediate():
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        yield


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        yield


def _consumer_for(runner):
    consumer = RunnerConsumer()
    consumer.runner = runner
    return consumer


# ---------------------------------------------------------------------------
# _handle_run_paused
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_paused_parks_run_without_ended_at(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        status=AgentRunStatus.RUNNING,
        prompt="working",
        started_at=timezone.now() - timezone.timedelta(minutes=2),
    )
    msg = {
        "run_id": str(run.id),
        "payload": {
            "status": "paused",
            "summary": "stuck",
            "autonomy": {"question_for_human": "which API surface?"},
        },
    }
    _consumer_for(online_runner)._handle_run_paused(online_runner, msg)
    run.refresh_from_db()
    assert run.status == AgentRunStatus.PAUSED_AWAITING_INPUT
    # Pause is non-terminal — ended_at must remain NULL.
    assert run.ended_at is None
    assert run.done_payload["autonomy"]["question_for_human"] == "which API surface?"


@pytest.mark.unit
def test_paused_escapes_agent_supplied_html_in_comment(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    """Agent payload is untrusted (upstream prompt can shape it). The
    comment we surface must escape HTML, never persist raw markup that
    could execute when rendered in the issue feed.
    """
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        status=AgentRunStatus.RUNNING,
        prompt="x",
        started_at=timezone.now(),
    )
    payload = {
        "summary": "<img src=x onerror=alert(1)>",
        "autonomy": {
            "question_for_human": "<script>alert('xss')</script>",
        },
    }
    _consumer_for(online_runner)._handle_run_paused(
        online_runner, {"run_id": str(run.id), "payload": payload}
    )

    comment = (
        IssueComment.objects.filter(issue=issue_in_progress)
        .order_by("-created_at")
        .first()
    )
    assert comment is not None
    html = comment.comment_html
    # No raw script/img tags reach storage — they only appear as escaped
    # text content. ``onerror=`` survives as a literal substring inside the
    # escaped form, which is harmless because it's no longer an attribute
    # on a real element.
    assert "<script>" not in html
    assert "<img" not in html
    # Escaped form is present — sanity check that escaping ran.
    assert "&lt;script&gt;" in html
    assert "&lt;img" in html


@pytest.mark.unit
def test_paused_skips_comment_when_no_question_or_summary(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        status=AgentRunStatus.RUNNING,
        prompt="x",
        started_at=timezone.now(),
    )
    _consumer_for(online_runner)._handle_run_paused(
        online_runner, {"run_id": str(run.id), "payload": {}}
    )
    assert IssueComment.objects.filter(issue=issue_in_progress).count() == 0


@pytest.mark.unit
def test_paused_sweep_creates_continuation_for_mid_run_comment(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    """Comments that arrived during RUNNING were skipped with
    'prior-run-active'. The pause transition is the symmetric recovery
    point — those comments must wake R_next.
    """
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        status=AgentRunStatus.RUNNING,
        prompt="x",
        started_at=timezone.now() - timezone.timedelta(minutes=5),
    )
    # Mid-run comment.
    with impersonate(create_user):
        IssueComment.objects.create(
            issue=issue_in_progress,
            project=issue_in_progress.project,
            workspace=workspace,
            actor=create_user,
            comment_html="<p>use option B</p>",
        )

    _consumer_for(online_runner)._handle_run_paused(
        online_runner,
        {
            "run_id": str(run.id),
            "payload": {"autonomy": {"question_for_human": "?"}},
        },
    )

    follow_up = (
        AgentRun.objects.filter(work_item=issue_in_progress, parent_run=run)
        .order_by("-created_at")
        .first()
    )
    assert follow_up is not None
    assert follow_up.status in (
        AgentRunStatus.QUEUED,
        AgentRunStatus.ASSIGNED,
    )


# ---------------------------------------------------------------------------
# _handle_resume_unavailable
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_resume_unavailable_drops_pin_and_requeues(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    parent = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        thread_id="sess_dead",
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
        prompt="prior",
        started_at=timezone.now() - timezone.timedelta(minutes=5),
    )
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        parent_run=parent,
        pinned_runner=online_runner,
        status=AgentRunStatus.ASSIGNED,
        prompt="continuation",
        assigned_at=timezone.now(),
    )
    _consumer_for(online_runner)._handle_resume_unavailable(
        online_runner, str(run.id)
    )
    run.refresh_from_db()
    parent.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.runner_id is None
    assert run.pinned_runner_id is None
    assert run.assigned_at is None
    # Parent's thread_id is cleared so the next dispatch doesn't hand the
    # same dead session id to a different runner.
    assert parent.thread_id == ""


@pytest.mark.unit
def test_resume_unavailable_noop_when_run_unknown(
    db, online_runner
):
    import uuid

    # Should not raise, should not touch any rows.
    _consumer_for(online_runner)._handle_resume_unavailable(
        online_runner, str(uuid.uuid4())
    )
