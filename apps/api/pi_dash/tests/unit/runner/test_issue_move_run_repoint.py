# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression tests for PDASHOSS01-65.

Moving an issue to another project must repoint the issue's outstanding
QUEUED agent run onto the target project's default pod (and clear any
``pinned_runner`` that lives in the old pod), otherwise the matcher — which
only drains runs whose ``pod`` matches an idle runner's pod — can never pick
the run up in the new project and it sits in the queue forever. Because a
QUEUED run also counts as the issue's single active run, no replacement run
gets created either, leaving the issue permanently unrunnable.
"""

from __future__ import annotations

from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.app.permissions import ROLE
from pi_dash.db.models import (
    Issue,
    Project,
    ProjectMember,
    State,
)
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.utils.issue_move import move_work_item_to_project


@pytest.fixture
def source_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Repoint Source",
        identifier="RPSRC",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(
        project=project,
        member=create_user,
        role=ROLE.ADMIN.value,
        is_active=True,
    )
    return project


@pytest.fixture
def target_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Repoint Target",
        identifier="RPDST",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(
        project=project,
        member=create_user,
        role=ROLE.ADMIN.value,
        is_active=True,
    )
    # The move short-circuits (400) unless the target has a non-triage default
    # workflow state to land the issue in.
    State.objects.create(
        name="Todo",
        project=project,
        workspace=workspace,
        group="unstarted",
        default=True,
        created_by=create_user,
    )
    return project


def _make_runner(user, workspace, pod, name):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now() - timezone.timedelta(seconds=1),
    )


def _make_issue(project, user):
    return Issue.objects.create(
        name="move target",
        project=project,
        workspace=project.workspace,
        created_by=user,
    )


def _move(workspace, source_project, target_project, issue, actor):
    return move_work_item_to_project(
        slug=workspace.slug,
        project_id=source_project.id,
        pk=issue.id,
        target_ref=target_project.identifier,
        actor=actor,
        origin="testserver",
    )


@pytest.fixture(autouse=True)
def _mock_celery():
    """The move enqueues activity/webhook Celery tasks; stub ``delay`` so the
    unit test doesn't need a broker (mirrors the ``mock_celery`` fixture)."""
    from unittest.mock import MagicMock

    with patch("celery.app.task.Task.delay") as mock_delay:
        mock_delay.return_value = MagicMock(id="mock-task-id")
        yield mock_delay


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    """The drain kicked on commit dispatches over the WS; stub it out."""
    with patch("pi_dash.runner.services.pubsub.send_to_runner") as mock:
        yield mock


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    """pytest-django rolls each test back, so ``on_commit`` never fires.
    Run the callbacks inline to exercise the post-move drain."""
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


@pytest.mark.unit
def test_move_repoints_queued_run_to_target_pod(db, create_user, workspace, source_project, target_project):
    source_pod = Pod.default_for_project(source_project)
    target_pod = Pod.default_for_project(target_project)
    source_runner = _make_runner(create_user, workspace, source_pod, "src-runner")

    issue = _make_issue(source_project, create_user)
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=source_pod,
        work_item=issue,
        status=AgentRunStatus.QUEUED,
        pinned_runner=source_runner,
        prompt="go",
    )

    _move(workspace, source_project, target_project, issue, create_user)

    run.refresh_from_db()
    # Repointed onto the new project's pod, and the old-pod pin is cleared so
    # the run is eligible for pickup in the new project.
    assert run.pod_id == target_pod.id
    assert run.pinned_runner_id is None


@pytest.mark.unit
def test_repointed_run_is_drainable_by_target_runner(db, create_user, workspace, source_project, target_project):
    source_pod = Pod.default_for_project(source_project)
    target_pod = Pod.default_for_project(target_project)
    _make_runner(create_user, workspace, source_pod, "src-runner")
    target_runner = _make_runner(create_user, workspace, target_pod, "dst-runner")

    issue = _make_issue(source_project, create_user)
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=source_pod,
        work_item=issue,
        status=AgentRunStatus.QUEUED,
        prompt="go",
    )

    _move(workspace, source_project, target_project, issue, create_user)

    run.refresh_from_db()
    # The post-move drain (fired on commit) hands the run to an idle runner in
    # the new project — no longer stuck in queue.
    assert run.pod_id == target_pod.id
    assert run.status == AgentRunStatus.ASSIGNED
    assert run.runner_id == target_runner.id


@pytest.mark.unit
def test_move_leaves_running_run_in_place(db, create_user, workspace, source_project, target_project):
    """A run already mid-flight on a live runner is not repointed — it finishes
    (or fails) on the original runner. Only QUEUED runs are stuck."""
    source_pod = Pod.default_for_project(source_project)
    source_runner = _make_runner(create_user, workspace, source_pod, "src-runner")

    issue = _make_issue(source_project, create_user)
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=source_pod,
        runner=source_runner,
        work_item=issue,
        status=AgentRunStatus.RUNNING,
        prompt="in-flight",
    )

    _move(workspace, source_project, target_project, issue, create_user)

    run.refresh_from_db()
    assert run.pod_id == source_pod.id
    assert run.runner_id == source_runner.id


@pytest.mark.unit
def test_move_leaves_terminal_run_in_place(db, create_user, workspace, source_project, target_project):
    """Completed runs are historical records and must not be repointed."""
    source_pod = Pod.default_for_project(source_project)

    issue = _make_issue(source_project, create_user)
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=source_pod,
        work_item=issue,
        status=AgentRunStatus.COMPLETED,
        prompt="done",
    )

    _move(workspace, source_project, target_project, issue, create_user)

    run.refresh_from_db()
    assert run.pod_id == source_pod.id
