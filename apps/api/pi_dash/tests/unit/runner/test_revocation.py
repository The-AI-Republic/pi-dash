# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``Runner.revoke()`` synchronous in-flight cleanup (design §7.5)."""

from __future__ import annotations

from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Connection,
    Pod,
    Runner,
    RunnerStatus,
)


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


def _make_connection(user, workspace, name="connection_test") -> Connection:
    return Connection.objects.create(
        workspace=workspace,
        created_by=user,
        name=name,
        secret_hash=f"sh-{name}",
        secret_fingerprint=name[:12].ljust(12, "x")[:12],
        enrolled_at=timezone.now(),
    )


def _make_runner(user, workspace, pod, name="r1"):
    connection = _make_connection(user, workspace, name=f"connection_{name}")
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        connection=connection,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with patch(
        "pi_dash.runner.services.pubsub.send_to_runner"
    ) as mock:
        yield mock


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    """Run ``transaction.on_commit`` callbacks inline in tests.

    pytest-django rolls back the wrapping transaction so post-commit hooks
    never fire; patching makes them run immediately so we can assert on
    drain-after-revoke behavior.
    """
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        yield


@pytest.mark.unit
def test_revoke_sets_status_and_timestamp(db, create_user, workspace, pod):
    r = _make_runner(create_user, workspace, pod)
    r.revoke()
    r.refresh_from_db()
    assert r.status == RunnerStatus.REVOKED
    assert r.revoked_at is not None


@pytest.mark.unit
def test_revoke_cancels_in_flight_assigned_run(
    db, create_user, workspace, pod
):
    r = _make_runner(create_user, workspace, pod)
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        runner=r,
        status=AgentRunStatus.ASSIGNED,
        prompt="x",
    )
    r.revoke()
    run.refresh_from_db()
    assert run.status == AgentRunStatus.CANCELLED
    assert run.ended_at is not None
    assert run.error == "runner revoked"


@pytest.mark.unit
def test_revoke_cancels_awaiting_approval_run(db, create_user, workspace, pod):
    r = _make_runner(create_user, workspace, pod)
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        runner=r,
        status=AgentRunStatus.AWAITING_APPROVAL,
        prompt="x",
    )
    r.revoke()
    run.refresh_from_db()
    assert run.status == AgentRunStatus.CANCELLED


@pytest.mark.unit
def test_revoke_leaves_terminal_runs_alone(db, create_user, workspace, pod):
    r = _make_runner(create_user, workspace, pod)
    done = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        runner=r,
        status=AgentRunStatus.COMPLETED,
        prompt="x",
    )
    r.revoke()
    done.refresh_from_db()
    assert done.status == AgentRunStatus.COMPLETED


@pytest.mark.unit
def test_revoke_refires_drain_for_affected_pod(
    db, create_user, workspace, pod
):
    """After revoke, if another runner exists in the pod, queued work should
    move to it via the post-commit drain."""
    revoked = _make_runner(create_user, workspace, pod, name="to-revoke")
    survivor = _make_runner(create_user, workspace, pod, name="survivor")
    in_flight = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        runner=revoked,
        status=AgentRunStatus.RUNNING,
        prompt="in-flight",
    )
    queued = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="queued",
    )

    revoked.revoke()

    in_flight.refresh_from_db()
    queued.refresh_from_db()
    assert in_flight.status == AgentRunStatus.CANCELLED
    # The post-commit drain should have picked up the queued run via the
    # survivor runner.
    assert queued.status == AgentRunStatus.ASSIGNED
    assert queued.runner_id == survivor.pk


@pytest.mark.unit
def test_revoke_is_noop_when_no_in_flight_runs(
    db, create_user, workspace, pod
):
    r = _make_runner(create_user, workspace, pod)
    # No AgentRun rows attached — revoke should still succeed.
    r.revoke()
    r.refresh_from_db()
    assert r.status == RunnerStatus.REVOKED


# ---------------------------------------------------------------------------
# Pin release on revoke (§5.7 of the design doc).
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_revoke_releases_pinned_queued_runs(
    db, create_user, workspace, project
):
    from django.utils import timezone

    from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod, Runner, RunnerStatus

    pod = Pod.default_for_project(project)
    connection = _make_connection(create_user, workspace, name="connection_agentX")
    runner = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        connection=connection,
        name="agentX",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )
    pinned_queued = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        prompt="waiting for agentX",
        status=AgentRunStatus.QUEUED,
        pinned_runner=runner,
    )

    runner.revoke()

    pinned_queued.refresh_from_db()
    # Pin dropped, but the run stays QUEUED so the pod can dispatch it
    # to anyone with a fresh-context fallback.
    assert pinned_queued.status == AgentRunStatus.QUEUED
    assert pinned_queued.pinned_runner_id is None
