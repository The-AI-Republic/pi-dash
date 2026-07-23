# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression tests for the cancellation-gated project-move handoff."""

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
from pi_dash.orchestration.service import (
    _pinned_runner_for,
    complete_project_move_handoff,
)
from pi_dash.runner.services import run_lifecycle
from pi_dash.utils.issue_move import IssueMoveError, move_work_item_to_project


@pytest.fixture
def source_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Repoint Source",
        identifier="RPSRC",
        repo_url="git@example.com:source/repo.git",
        base_branch="source-main",
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
        repo_url="git@example.com:target/repo.git",
        base_branch="target-main",
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
def test_move_replaces_queued_run_with_fresh_target_run(db, create_user, workspace, source_project, target_project):
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
        run_config={
            "repo_url": source_project.repo_url,
            "repo_ref": source_project.base_branch,
            "model": "test-model",
        },
    )

    with patch(
        "pi_dash.orchestration.signals.handle_issue_state_transition"
    ) as transition:
        _move(
            workspace,
            source_project,
            target_project,
            issue,
            create_user,
        )

    assert transition.call_args.kwargs["dispatch_immediate"] is False

    run.refresh_from_db()
    replacement = AgentRun.objects.exclude(pk=run.pk).get(work_item=issue)

    assert run.status == AgentRunStatus.CANCELLED
    assert run.pod_id == source_pod.id
    assert replacement.parent_run_id == run.id
    assert replacement.pod_id == target_pod.id
    assert replacement.pinned_runner_id is None
    assert replacement.run_config["repo_url"] == target_project.repo_url
    assert replacement.run_config["repo_ref"] == target_project.base_branch
    assert replacement.run_config["model"] == "test-model"


@pytest.mark.unit
def test_replacement_run_is_drainable_by_target_runner(db, create_user, workspace, source_project, target_project):
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
    replacement = AgentRun.objects.exclude(pk=run.pk).get(work_item=issue)
    assert run.status == AgentRunStatus.CANCELLED
    assert replacement.pod_id == target_pod.id
    assert replacement.status == AgentRunStatus.ASSIGNED
    assert replacement.runner_id == target_runner.id


@pytest.mark.unit
def test_move_cancels_running_run_then_creates_target_handoff(
    db,
    create_user,
    workspace,
    source_project,
    target_project,
    _stub_send_to_runner,
):
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
    assert run.status == AgentRunStatus.CANCEL_REQUESTED
    assert run.pod_id == source_pod.id
    assert run.runner_id == source_runner.id
    assert AgentRun.objects.filter(work_item=issue).count() == 1
    _stub_send_to_runner.assert_called_once()
    cancel = _stub_send_to_runner.call_args.args[1]
    assert cancel["type"] == "cancel"
    assert cancel["run_id"] == str(run.id)

    # The target run does not exist until the source runner acknowledges that
    # its agent process stopped.
    run_lifecycle.finalize_run_terminal(
        source_runner,
        run.id,
        AgentRunStatus.CANCELLED,
    )

    run.refresh_from_db()
    replacement = AgentRun.objects.exclude(pk=run.pk).get(work_item=issue)
    assert run.status == AgentRunStatus.CANCELLED
    assert replacement.parent_run_id == run.id
    assert replacement.pod_id == Pod.default_for_project(target_project).id
    assert replacement.pinned_runner_id is None
    assert replacement.run_config["repo_url"] == target_project.repo_url

    # Retried terminal delivery / recovery callbacks are idempotent.
    assert complete_project_move_handoff(run.id).id == replacement.id
    assert AgentRun.objects.filter(work_item=issue).count() == 2


@pytest.mark.unit
def test_runner_revoke_completes_pending_project_move_handoff(
    db,
    create_user,
    workspace,
    source_project,
    target_project,
):
    source_pod = Pod.default_for_project(source_project)
    source_runner = _make_runner(
        create_user,
        workspace,
        source_pod,
        "revoked-src-runner",
    )
    issue = _make_issue(source_project, create_user)
    source_run = AgentRun.objects.create(
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
    source_run.refresh_from_db()
    assert source_run.status == AgentRunStatus.CANCEL_REQUESTED

    # Revocation closes busy rows directly instead of receiving a runner
    # lifecycle acknowledgement. It must still release the move handoff.
    source_runner.revoke()

    source_run.refresh_from_db()
    replacement = AgentRun.objects.exclude(pk=source_run.pk).get(work_item=issue)
    assert source_run.status == AgentRunStatus.CANCELLED
    assert replacement.parent_run_id == source_run.id
    assert replacement.pod_id == Pod.default_for_project(target_project).id
    assert replacement.pinned_runner_id is None


@pytest.mark.unit
def test_move_leaves_terminal_run_in_place(db, create_user, workspace, source_project, target_project):
    """Completed runs are historical records and must not be repointed."""
    source_pod = Pod.default_for_project(source_project)
    source_runner = _make_runner(
        create_user,
        workspace,
        source_pod,
        "historical-src-runner",
    )

    issue = _make_issue(source_project, create_user)
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=source_pod,
        runner=source_runner,
        work_item=issue,
        status=AgentRunStatus.COMPLETED,
        prompt="done",
    )

    _move(workspace, source_project, target_project, issue, create_user)

    run.refresh_from_db()
    assert run.pod_id == source_pod.id
    assert AgentRun.objects.filter(work_item=issue).count() == 1
    assert _pinned_runner_for(
        run,
        Pod.default_for_project(target_project),
    ) is None


@pytest.mark.unit
def test_move_without_active_run_does_not_require_target_pod(
    db,
    create_user,
    workspace,
    source_project,
    target_project,
):
    target_pod = Pod.default_for_project(target_project)
    target_pod.deleted_at = timezone.now()
    target_pod.save(update_fields=["deleted_at"])
    issue = _make_issue(source_project, create_user)

    with patch(
        "pi_dash.orchestration.signals.handle_issue_state_transition"
    ) as transition:
        moved = _move(
            workspace,
            source_project,
            target_project,
            issue,
            create_user,
        )

    assert moved.project_id == target_project.id
    assert moved.assigned_pod_id is None
    assert transition.call_args.kwargs["dispatch_immediate"] is True


@pytest.mark.unit
def test_move_refuses_to_hide_multiple_executing_runs(
    db,
    create_user,
    workspace,
    source_project,
    target_project,
):
    source_pod = Pod.default_for_project(source_project)
    first_runner = _make_runner(
        create_user,
        workspace,
        source_pod,
        "src-runner-1",
    )
    second_runner = _make_runner(
        create_user,
        workspace,
        source_pod,
        "src-runner-2",
    )
    issue = _make_issue(source_project, create_user)
    for runner in (first_runner, second_runner):
        AgentRun.objects.create(
            workspace=workspace,
            created_by=create_user,
            pod=source_pod,
            runner=runner,
            work_item=issue,
            status=AgentRunStatus.RUNNING,
            prompt="in-flight",
        )

    with pytest.raises(IssueMoveError) as exc_info:
        _move(
            workspace,
            source_project,
            target_project,
            issue,
            create_user,
        )

    assert exc_info.value.status_code == 409
    issue.refresh_from_db()
    assert issue.project_id == source_project.id
    assert set(
        AgentRun.objects.filter(work_item=issue).values_list(
            "status",
            flat=True,
        )
    ) == {AgentRunStatus.RUNNING}
