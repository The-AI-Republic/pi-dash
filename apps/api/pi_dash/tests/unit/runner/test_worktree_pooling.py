# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Worktree-pooling cloud-side contract tests.

Covers the ``queued`` lifecycle verb, the WAITING_FOR_WORKTREE status-set
membership (busy / non-terminal / reaper), user-cancel from the waiting
state, and session-open redelivery. See
``.ai_design/worktree_pooling/design.md`` §6.1–§6.3 and the implementation
plan phases 4–5.
"""

from __future__ import annotations

import uuid as _uuid
from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import matcher, session_service, tokens


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def enrolled_runner(db, create_user, workspace, pod):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="agentR",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
        refresh_token_generation=1,
        enrolled_at=timezone.now(),
    )


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


def _post_queued(api_client, run, token, position, key=None):
    headers = {"HTTP_AUTHORIZATION": f"Bearer {token}"}
    if key is not None:
        headers["HTTP_IDEMPOTENCY_KEY"] = key
    return api_client.post(
        f"/api/v1/runner/runs/{run.id}/queued/",
        {"queue_position": position},
        format="json",
        **headers,
    )


# ---------------------------------------------------------------------------
# RunQueuedEndpoint transition matrix
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_queued_assigned_to_waiting_with_position(
    db, api_client, runner_token, assigned_run
):
    resp = _post_queued(api_client, assigned_run, runner_token, 3)
    assert resp.status_code == 200, resp.data
    assert resp.data.get("ok") is True
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.WAITING_FOR_WORKTREE
    assert assigned_run.queue_position == 3


@pytest.mark.unit
def test_queued_waiting_to_waiting_refreshes_position(
    db, api_client, runner_token, assigned_run
):
    assigned_run.status = AgentRunStatus.WAITING_FOR_WORKTREE
    assigned_run.queue_position = 5
    assigned_run.save(update_fields=["status", "queue_position"])

    resp = _post_queued(api_client, assigned_run, runner_token, 2)
    assert resp.status_code == 200, resp.data
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.WAITING_FOR_WORKTREE
    assert assigned_run.queue_position == 2


@pytest.mark.unit
def test_queued_does_not_regress_running_run(
    db, api_client, runner_token, assigned_run
):
    """A late/duplicate queued post against a RUNNING run is acknowledged and
    dropped — it must never pull a live run back to WAITING_FOR_WORKTREE."""
    assigned_run.status = AgentRunStatus.RUNNING
    assigned_run.started_at = timezone.now()
    assigned_run.save(update_fields=["status", "started_at"])

    resp = _post_queued(api_client, assigned_run, runner_token, 1)
    assert resp.status_code == 200, resp.data
    assert resp.data.get("ignored") is True
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.RUNNING
    assert assigned_run.queue_position is None


@pytest.mark.unit
def test_queued_terminal_is_acknowledged_and_dropped(
    db, api_client, runner_token, assigned_run
):
    assigned_run.status = AgentRunStatus.COMPLETED
    assigned_run.ended_at = timezone.now()
    assigned_run.save(update_fields=["status", "ended_at"])

    resp = _post_queued(api_client, assigned_run, runner_token, 1)
    assert resp.status_code == 200, resp.data
    assert resp.data.get("terminal") is True
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.COMPLETED
    assert assigned_run.queue_position is None


@pytest.mark.unit
def test_queued_dedupes_duplicate(db, api_client, runner_token, assigned_run):
    msg_id = _uuid.uuid4().hex
    first = _post_queued(api_client, assigned_run, runner_token, 4, key=msg_id)
    second = _post_queued(api_client, assigned_run, runner_token, 1, key=msg_id)
    assert first.status_code == 200
    assert second.status_code == 200
    assert second.data.get("duplicate") is True
    assigned_run.refresh_from_db()
    # The duplicate must not have applied the second (smaller) position.
    assert assigned_run.queue_position == 4


@pytest.mark.unit
def test_queued_rejects_other_runner(
    db, api_client, runner_token, assigned_run, create_user, workspace, pod
):
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
    resp = _post_queued(api_client, other_run, runner_token, 1)
    assert resp.status_code == 403
    assert resp.data["error"] == "run_not_owned_by_runner"


# ---------------------------------------------------------------------------
# Status-set membership: blocks pod deletion / counts as busy
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_waiting_status_is_busy_and_non_terminal():
    assert AgentRunStatus.WAITING_FOR_WORKTREE in matcher.BUSY_STATUSES
    assert AgentRunStatus.WAITING_FOR_WORKTREE in matcher.NON_TERMINAL_STATUSES


@pytest.mark.unit
def test_waiting_run_makes_runner_busy(db, create_user, workspace, pod, enrolled_runner):
    """A waiting run occupies its single-tenant runner — the matcher must not
    hand it more work."""
    from django.db import transaction

    AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        runner=enrolled_runner,
        prompt="waiting",
        status=AgentRunStatus.WAITING_FOR_WORKTREE,
        assigned_at=timezone.now(),
    )
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None


# ---------------------------------------------------------------------------
# Reaper integration
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_reaper_spares_reported_waiting_run(
    db, create_user, workspace, pod, enrolled_runner
):
    """A waiting run the runner DOES report as in_flight is spared."""
    run = AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        runner=enrolled_runner,
        prompt="waiting",
        status=AgentRunStatus.WAITING_FOR_WORKTREE,
        assigned_at=timezone.now() - timezone.timedelta(minutes=10),
    )
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        session_service.reap_stale_busy_runs(
            enrolled_runner,
            {"in_flight_run": str(run.id), "ts": timezone.now().isoformat()},
        )
    run.refresh_from_db()
    assert run.status == AgentRunStatus.WAITING_FOR_WORKTREE


@pytest.mark.unit
def test_reaper_fails_unreported_waiting_run(
    db, create_user, workspace, pod, enrolled_runner
):
    """A waiting run older than the grace that the runner does NOT report is
    reaped — the daemon truly lost it (crash without restart)."""
    run = AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        runner=enrolled_runner,
        prompt="waiting",
        status=AgentRunStatus.WAITING_FOR_WORKTREE,
        assigned_at=timezone.now() - timezone.timedelta(minutes=10),
    )
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        session_service.reap_stale_busy_runs(
            enrolled_runner,
            {"in_flight_run": None, "ts": timezone.now().isoformat()},
        )
    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert run.ended_at is not None


# ---------------------------------------------------------------------------
# User cancel from WAITING_FOR_WORKTREE
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_cancel_from_waiting_for_worktree(db, api_client, create_user, assigned_run):
    assigned_run.status = AgentRunStatus.WAITING_FOR_WORKTREE
    assigned_run.queue_position = 1
    assigned_run.save(update_fields=["status", "queue_position"])

    api_client.force_authenticate(user=create_user)
    with (
        patch("pi_dash.runner.views.runs.send_to_runner") as send,
        patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()),
    ):
        resp = api_client.post(
            f"/api/runners/runs/{assigned_run.id}/cancel/",
            {"reason": "user cancelled"},
            format="json",
        )
    assert resp.status_code == 200, getattr(resp, "data", resp)
    assigned_run.refresh_from_db()
    assert assigned_run.status == AgentRunStatus.CANCELLED
    assert assigned_run.ended_at is not None
    # The cancel frame is delivered to the runner so it can dequeue.
    assert send.called
    sent = send.call_args[0][1]
    assert sent["type"] == "cancel"
    assert sent["run_id"] == str(assigned_run.id)


# ---------------------------------------------------------------------------
# Session-open redelivery (Phase 5)
# ---------------------------------------------------------------------------


def _open_session(api_client, runner, token, *, in_flight=None):
    return api_client.post(
        f"/api/v1/runner/runners/{runner.id}/sessions/",
        {
            "version": "test",
            "os": "linux",
            "arch": "x86_64",
            "status": "online",
            "in_flight_run": in_flight,
        },
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {token}",
    )


@pytest.mark.unit
def test_session_open_redelivers_unreported_waiting_run(
    db, api_client, runner_token, enrolled_runner, assigned_run
):
    assigned_run.status = AgentRunStatus.WAITING_FOR_WORKTREE
    assigned_run.run_config = {
        "repo_url": "git@example.com:x/y.git",
        "repo_ref": "main",
        "git_work_branch": "pidash/run",
    }
    assigned_run.save(update_fields=["status", "run_config"])

    resp = _open_session(api_client, enrolled_runner, runner_token, in_flight=None)
    assert resp.status_code == 201, resp.data
    redeliver = resp.data.get("redeliver")
    assert redeliver is not None
    assert redeliver["type"] == "assign"
    assert redeliver["run_id"] == str(assigned_run.id)
    assert redeliver["repo_url"] == "git@example.com:x/y.git"
    assert redeliver["git_work_branch"] == "pidash/run"


@pytest.mark.unit
def test_session_open_no_redeliver_when_reported_in_flight(
    db, api_client, runner_token, enrolled_runner, assigned_run
):
    assigned_run.status = AgentRunStatus.WAITING_FOR_WORKTREE
    assigned_run.save(update_fields=["status"])

    resp = _open_session(
        api_client, enrolled_runner, runner_token, in_flight=str(assigned_run.id)
    )
    assert resp.status_code == 201, resp.data
    assert resp.data.get("redeliver") is None
    # The reported run still gets a resume ack.
    assert resp.data.get("resume_ack") is not None


@pytest.mark.unit
def test_session_open_no_redeliver_for_running_run(
    db, api_client, runner_token, enrolled_runner, assigned_run
):
    """RUNNING runs the daemon lost are the reaper's job, never redelivered."""
    assigned_run.status = AgentRunStatus.RUNNING
    assigned_run.started_at = timezone.now()
    assigned_run.save(update_fields=["status", "started_at"])

    resp = _open_session(api_client, enrolled_runner, runner_token, in_flight=None)
    assert resp.status_code == 201, resp.data
    assert resp.data.get("redeliver") is None


# ---------------------------------------------------------------------------
# free_worktrees capacity hint persistence (Phase 7)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_poll_persists_free_worktrees(db, api_client, runner_token, enrolled_runner):
    open_resp = _open_session(api_client, enrolled_runner, runner_token, in_flight=None)
    assert open_resp.status_code == 201, open_resp.data
    sid = open_resp.data["session_id"]

    with (
        patch("pi_dash.runner.views.sessions.outbox.is_pel_drained", return_value=True),
        patch("pi_dash.runner.views.sessions.outbox.read_for_session", return_value=[]),
        patch("pi_dash.runner.views.sessions.outbox.mark_pel_drained"),
        patch("pi_dash.runner.views.sessions.outbox.ack_for_session"),
        patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()),
    ):
        poll_resp = api_client.post(
            f"/api/v1/runner/runners/{enrolled_runner.id}/sessions/{sid}/poll",
            {
                "ack": [],
                "status": {
                    "status": "online",
                    "free_worktrees": 1,
                    "ts": timezone.now().isoformat(),
                },
            },
            format="json",
            HTTP_AUTHORIZATION=f"Bearer {runner_token}",
        )
    assert poll_resp.status_code == 200, poll_resp.data
    enrolled_runner.refresh_from_db()
    assert enrolled_runner.free_worktrees == 1


@pytest.mark.unit
def test_parse_free_worktrees_coercion():
    assert session_service.parse_free_worktrees(None) is None
    assert session_service.parse_free_worktrees("bad") is None
    assert session_service.parse_free_worktrees(-1) is None
    assert session_service.parse_free_worktrees(0) == 0
    assert session_service.parse_free_worktrees("3") == 3
