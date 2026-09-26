# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression: a wedged BUSY runner is released when its run finalizes.

PDASHOSS01-231: when a daemon restarts mid-run (e.g. auto-update), the
shutdown drain finalizes the run as FAILED ("daemon shutdown requested"),
but nothing flipped the Runner row back from BUSY. The matcher only ever
assigns ONLINE runners, so runs pinned to the wedged runner (session
resume continuations) sat QUEUED indefinitely while it heartbeated
normally.

These tests pin the two reconciliation points:

1. Every run-finalization path releases the runner — BUSY flips to ONLINE
   as soon as no BUSY_STATUSES run remains, and the follow-up drain can
   assign a run pinned to it.
2. Session open reconciles: a reopening daemon that reports no in-flight
   run and has no non-terminal run in the DB gets its stale BUSY cleared.

The per-poll self-report path (``_poll_bookkeeping`` writing ONLINE from
``status.status``) is covered by ``test_poll_heartbeat_drain.py`` and the
session-open dispatch test in ``test_https_transport_run_endpoints.py``.
"""

from __future__ import annotations

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
from pi_dash.runner.services import tokens
from pi_dash.runner.services.agent_run_finalization import finalize_agent_run
from pi_dash.runner.services.matcher import release_runner_if_idle
from pi_dash.runner.services.run_lifecycle import finalize_run_terminal


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


def _make_runner(user, workspace, pod, name="wedged", status=RunnerStatus.BUSY):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=status,
        last_heartbeat_at=timezone.now(),
        refresh_token_generation=1,
        enrolled_at=timezone.now(),
    )


def _make_run(user, workspace, pod, runner, *, status=AgentRunStatus.RUNNING, **extra):
    return AgentRun.objects.create(
        workspace=workspace,
        owner=user,
        created_by=user,
        pod=pod,
        runner=runner,
        status=status,
        prompt="test",
        assigned_at=timezone.now(),
        started_at=timezone.now(),
        **extra,
    )


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    """Finalization schedules its effects via on_commit; tests run outside
    an atomic block so the callbacks would otherwise never fire."""
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


# ---------------------------------------------------------------------------
# release_runner_if_idle
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_release_flips_busy_runner_with_no_runs(db, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod)
    assert release_runner_if_idle(runner.id) is True
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.ONLINE


@pytest.mark.unit
def test_release_keeps_busy_runner_with_active_run(db, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod)
    _make_run(create_user, workspace, pod, runner)
    assert release_runner_if_idle(runner.id) is False
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.BUSY


@pytest.mark.unit
@pytest.mark.parametrize(
    "status", [RunnerStatus.OFFLINE, RunnerStatus.REVOKED, RunnerStatus.ONLINE]
)
def test_release_only_touches_busy(db, create_user, workspace, pod, status):
    """OFFLINE / REVOKED are stronger states owned by their own
    transitions; a release must never resurrect them."""
    runner = _make_runner(create_user, workspace, pod, status=status)
    assert release_runner_if_idle(runner.id) is False
    runner.refresh_from_db()
    assert runner.status == status


@pytest.mark.unit
def test_release_ignores_terminal_and_paused_runs(db, create_user, workspace, pod):
    """Terminal rows and PAUSED_AWAITING_INPUT don't occupy the runner
    (the matcher's BUSY_STATUSES excludes them), so they must not block
    the release."""
    runner = _make_runner(create_user, workspace, pod)
    _make_run(create_user, workspace, pod, runner, status=AgentRunStatus.FAILED)
    _make_run(
        create_user, workspace, pod, runner, status=AgentRunStatus.PAUSED_AWAITING_INPUT
    )
    assert release_runner_if_idle(runner.id) is True
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.ONLINE


# ---------------------------------------------------------------------------
# Finalization releases the runner (the daemon-shutdown failure path)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_shutdown_failure_finalize_releases_runner(db, create_user, workspace, pod):
    """The exact prod wedge: RunFailed{DaemonRestart} finalizes the run
    but the runner stayed BUSY forever. Finalization must now release it."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.FAILED,
        error_detail="daemon shutdown requested",
    )

    run.refresh_from_db()
    runner.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert runner.status == RunnerStatus.ONLINE


@pytest.mark.unit
def test_reaped_finalize_releases_runner(db, create_user, workspace, pod):
    """The heartbeat reaper funnels through finalize_agent_run too; a run
    it fails must release the runner the same way."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner)

    assert finalize_agent_run(
        run.id,
        AgentRunStatus.FAILED,
        updates={"error": "reaped by heartbeat", "error_code": "heartbeat_reaped"},
        expected_runner_id=runner.id,
    )

    runner.refresh_from_db()
    assert runner.status == RunnerStatus.ONLINE


@pytest.mark.unit
def test_finalize_keeps_runner_busy_while_another_run_active(
    db, create_user, workspace, pod
):
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner)
    _make_run(create_user, workspace, pod, runner, status=AgentRunStatus.ASSIGNED)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.FAILED,
        error_detail="daemon shutdown requested",
    )

    runner.refresh_from_db()
    assert runner.status == RunnerStatus.BUSY


@pytest.mark.unit
def test_release_then_drain_assigns_pinned_run(db, create_user, workspace, pod):
    """End-to-end through the finalization effects: the failing run ends,
    the runner is released, and the drain that follows in the same effects
    pass assigns the QUEUED run pinned to that runner (the continuation
    that used to sit queued for 47 minutes)."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner)
    pinned = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        pinned_runner=runner,
        prompt="continuation",
        status=AgentRunStatus.QUEUED,
    )

    with patch("pi_dash.runner.services.pubsub.send_to_runner") as mock_send:
        finalize_run_terminal(
            runner,
            run.id,
            AgentRunStatus.FAILED,
            error_detail="daemon shutdown requested",
        )

    runner.refresh_from_db()
    pinned.refresh_from_db()
    assert runner.status == RunnerStatus.ONLINE
    assert pinned.status == AgentRunStatus.ASSIGNED
    assert pinned.runner_id == runner.id
    assert mock_send.called


# ---------------------------------------------------------------------------
# Session open reconciles a stale BUSY
# ---------------------------------------------------------------------------


def _open_session(api_client, runner, token, *, in_flight_run=None):
    return api_client.post(
        f"/api/v1/runner/runners/{runner.id}/sessions/",
        {
            "version": "0.1.27",
            "os": "macos",
            "arch": "aarch64",
            "status": "idle",
            "in_flight_run": in_flight_run,
        },
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {token}",
    )


def _mint(runner):
    return tokens.mint_access_token(
        runner_id=str(runner.id),
        user_id=str(runner.owner_id),
        workspace_id=str(runner.workspace_id),
        rtg=1,
    ).raw


@pytest.mark.unit
def test_session_open_releases_stale_busy_runner(
    db, api_client, create_user, workspace, pod
):
    """A daemon restart reopens the session reporting no in-flight run;
    with no non-terminal run in the DB the stale BUSY must clear so the
    matcher can see the runner again."""
    runner = _make_runner(create_user, workspace, pod)
    _make_run(create_user, workspace, pod, runner, status=AgentRunStatus.FAILED)

    resp = _open_session(api_client, runner, _mint(runner))

    assert resp.status_code == 201, resp.data
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.ONLINE


@pytest.mark.unit
def test_session_open_keeps_busy_when_daemon_reports_in_flight(
    db, api_client, create_user, workspace, pod
):
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner)

    resp = _open_session(api_client, runner, _mint(runner), in_flight_run=str(run.id))

    assert resp.status_code == 201, resp.data
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.BUSY


@pytest.mark.unit
def test_session_open_keeps_busy_with_redeliverable_run(
    db, api_client, create_user, workspace, pod
):
    """An ASSIGNED run the daemon doesn't report is spared by the
    session-open reap for redelivery — it still occupies the runner, so
    the stale-BUSY release must not fire."""
    runner = _make_runner(create_user, workspace, pod)
    _make_run(create_user, workspace, pod, runner, status=AgentRunStatus.ASSIGNED)

    resp = _open_session(api_client, runner, _mint(runner))

    assert resp.status_code == 201, resp.data
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.BUSY
