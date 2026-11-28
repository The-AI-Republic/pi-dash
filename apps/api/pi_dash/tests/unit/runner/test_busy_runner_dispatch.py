# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Queued runs behind a busy runner (PDASHOSS01-419).

A project scaled from one runner to several: runs queued while the only
runner was busy must reach a runner that joins the pod, and
``dispatch_wait.reason`` must say ``runner_busy`` (not ``runner_offline``)
for a pinned runner that is online and working.
"""

from __future__ import annotations

from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import State
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    PinKind,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services.issue_assignment import (
    build_dispatch_wait,
    dedicated_run_refusal,
    runner_is_offline,
)
from pi_dash.runner.services.matcher import drain_for_runner_by_id


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


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with patch("pi_dash.runner.services.pubsub.send_to_runner") as mock:
        yield mock


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, *a, **k: fn()):
        yield


def _make_runner(user, workspace, pod, name):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _make_issue(workspace, project, state, user, *, runner=None, name="An issue"):
    from crum import impersonate

    with impersonate(user):
        return Issue.objects.create(
            name=name,
            project=project,
            workspace=workspace,
            state=state,
            created_by=user,
            assigned_runner=runner,
        )


def _occupy(runner, issue, user, pod):
    """Give ``runner`` a RUNNING run on ``issue``, as the poll reports it."""
    run = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=user,
        pod=pod,
        work_item=issue,
        runner=runner,
        owner=user,
        status=AgentRunStatus.RUNNING,
        assigned_at=timezone.now(),
        prompt="busy",
    )
    Runner.objects.filter(pk=runner.pk).update(status=RunnerStatus.BUSY)
    runner.refresh_from_db()
    return run


@pytest.mark.unit
def test_never_run_issues_queued_behind_busy_runner_go_to_new_runner(db, workspace, project, create_user, pod, state):
    from pi_dash.orchestration.scheduling import dispatch_run_ai_run_with_reason

    first = _make_runner(create_user, workspace, pod, "first")
    _occupy(first, _make_issue(workspace, project, state, create_user, name="busy"), create_user, pod)

    queued = []
    for name in ("new-1", "new-2"):
        issue = _make_issue(workspace, project, state, create_user, name=name)
        run, reason = dispatch_run_ai_run_with_reason(issue, actor=create_user)
        assert run is not None, reason
        run.refresh_from_db()
        assert run.status == AgentRunStatus.QUEUED
        # No previous run: nothing to be local to, so no pin.
        assert run.pinned_runner_id is None
        assert build_dispatch_wait(run) is None
        queued.append(run)

    second = _make_runner(create_user, workspace, pod, "second")
    # The new runner's first poll drains it.
    assert drain_for_runner_by_id(second.pk) is True

    assigned = AgentRun.objects.filter(pk__in=[r.pk for r in queued], runner=second)
    assert assigned.count() == 1
    assert assigned.get().status == AgentRunStatus.ASSIGNED


@pytest.mark.unit
def test_locality_pin_on_busy_runner_goes_to_new_runner(db, workspace, project, create_user, pod, state):
    """A locality pin is a hint: an idle pod runner takes it while its runner works."""
    first = _make_runner(create_user, workspace, pod, "first")
    issue = _make_issue(workspace, project, state, create_user)
    parent = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        runner=first,
        owner=create_user,
        status=AgentRunStatus.COMPLETED,
        assigned_at=timezone.now(),
        thread_id="thread-on-first",
        prompt="earlier",
    )
    _occupy(first, _make_issue(workspace, project, state, create_user, name="busy"), create_user, pod)
    queued = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        parent_run=parent,
        pinned_runner=first,
        pin_kind=PinKind.LOCALITY,
        status=AgentRunStatus.QUEUED,
        prompt="continue",
    )
    assert build_dispatch_wait(queued)["reason"] == "runner_busy"

    second = _make_runner(create_user, workspace, pod, "second")
    assert drain_for_runner_by_id(second.pk) is True

    queued.refresh_from_db()
    parent.refresh_from_db()
    assert queued.runner_id == second.pk
    assert queued.status == AgentRunStatus.ASSIGNED
    assert queued.pinned_runner_id is None
    # The new runner has no session of the parent's to resume.
    assert parent.thread_id == ""


@pytest.mark.unit
def test_dedicated_pin_on_busy_runner_stays(db, workspace, project, create_user, pod, state):
    """A dedicated pin is the issue's choice: another runner never takes it."""
    first = _make_runner(create_user, workspace, pod, "first")
    issue = _make_issue(workspace, project, state, create_user, runner=first)
    _occupy(first, _make_issue(workspace, project, state, create_user, name="busy"), create_user, pod)
    queued = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        pinned_runner=first,
        pin_kind=PinKind.DEDICATED,
        status=AgentRunStatus.QUEUED,
        prompt="dedicated",
    )
    second = _make_runner(create_user, workspace, pod, "second")
    assert drain_for_runner_by_id(second.pk) is False
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.QUEUED
    assert queued.pinned_runner_id == first.pk


@pytest.mark.unit
def test_busy_online_runner_reports_runner_busy(db, workspace, project, create_user, pod, state):
    runner = _make_runner(create_user, workspace, pod, "worker")
    issue = _make_issue(workspace, project, state, create_user, runner=runner)
    _occupy(runner, _make_issue(workspace, project, state, create_user, name="busy"), create_user, pod)
    assert runner.status == RunnerStatus.BUSY
    assert runner_is_offline(runner) is False

    queued = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        pinned_runner=runner,
        pin_kind=PinKind.DEDICATED,
        status=AgentRunStatus.QUEUED,
        prompt="waits",
    )
    wait = build_dispatch_wait(queued, viewer=create_user)
    assert wait["reason"] == "runner_busy"
    # Busy is never a refusal (§11.1).
    assert dedicated_run_refusal(issue, human=True, user=create_user)[0] is None


@pytest.mark.unit
def test_busy_status_with_stale_heartbeat_is_offline(db, workspace, project, create_user, pod):
    runner = _make_runner(create_user, workspace, pod, "gone")
    Runner.objects.filter(pk=runner.pk).update(
        status=RunnerStatus.BUSY,
        last_heartbeat_at=timezone.now() - timezone.timedelta(minutes=10),
    )
    runner.refresh_from_db()
    assert runner_is_offline(runner) is True
