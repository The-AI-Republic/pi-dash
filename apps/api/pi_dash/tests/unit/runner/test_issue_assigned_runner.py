# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Model contract for the dedicated-runner columns (design §6, §18 Model row).

Covers: I1 pod-is-set check, I5 dedicated-pin-is-local check, no executor
tie (I2 removed), SET_NULL on runner delete, defaults including
``fail_to_backlog`` on projects.
"""

from __future__ import annotations

import pytest
from django.db import IntegrityError, transaction
from django.utils import timezone

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import State
from pi_dash.runner.models import AgentRun, AgentRunStatus, PinKind, Pod, Runner, RunnerStatus


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


def _make_runner(user, workspace, pod, name="r1"):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _make_state(workspace, project):
    return State.objects.create(
        name="Backlog", group="backlog", workspace=workspace, project=project,
        sequence=1.0, default=True,
    )


@pytest.mark.unit
def test_new_rows_take_dedicated_defaults(db, workspace, project, create_user, pod):
    assert project.dedicated_runner_lost_action == "fail_to_backlog"
    state = _make_state(workspace, project)
    issue = Issue.objects.create(
        name="An issue", project=project, workspace=workspace,
        state=state, created_by=create_user,
    )
    assert issue.assigned_runner_id is None
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
    )
    assert run.pin_kind == PinKind.LOCALITY == "locality"


@pytest.mark.unit
def test_issue_dedicated_runner_needs_pod(db, workspace, project, create_user, pod):
    """I1: a dedicated runner without a pod is rejected by the check."""
    state = _make_state(workspace, project)
    issue = Issue.objects.create(
        name="An issue", project=project, workspace=workspace,
        state=state, created_by=create_user,
    )
    issue.assigned_pod_id = None
    issue.assigned_runner = _make_runner(create_user, workspace, pod)
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            issue.save()
    issue.refresh_from_db()
    assert issue.assigned_pod_id is not None  # the check rejected the write
    assert issue.assigned_runner_id is None


@pytest.mark.unit
def test_dedicated_run_accepts_dedicated_issue(db, workspace, project, create_user, pod):
    state = _make_state(workspace, project)
    runner = _make_runner(create_user, workspace, pod)
    issue = Issue.objects.create(
        name="An issue", project=project, workspace=workspace,
        state=state, created_by=create_user,
        assigned_pod=pod, assigned_runner=runner,
    )
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    assert run.pin_kind == "dedicated"


@pytest.mark.unit
def test_dedicated_pin_on_cloud_run_rejected(db, workspace, project, create_user, pod):
    """I5: a dedicated pin requires ``local_runner`` executor."""
    state = _make_state(workspace, project)
    issue = Issue.objects.create(
        name="An issue", project=project, workspace=workspace,
        state=state, created_by=create_user,
    )
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            AgentRun.objects.create(
                workspace=workspace, created_by=create_user, pod=pod,
                work_item=issue, status=AgentRunStatus.QUEUED,
                pin_kind=PinKind.DEDICATED, executor_kind="cloud_agent",
            )


@pytest.mark.unit
def test_assigned_runner_set_null_on_runner_delete(db, workspace, project, create_user, pod):
    state = _make_state(workspace, project)
    runner = _make_runner(create_user, workspace, pod)
    issue = Issue.objects.create(
        name="An issue", project=project, workspace=workspace,
        state=state, created_by=create_user,
        assigned_pod=pod, assigned_runner=runner,
    )
    runner_id = runner.id
    Runner.objects.filter(id=runner_id).delete()
    issue.refresh_from_db()
    assert issue.assigned_runner_id is None
