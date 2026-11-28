# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dedicated-runner inputs on the assistant issue tools (HA-5, design §14.6).

``create_issue``/``update_issue`` accept ``assigned_runner``/``assigned_pod``
(UUID or name) through the shared service; E-codes pass through to the
model. ``get_issue`` returns the assignment, its detail and the waiting
state.
"""

from __future__ import annotations

from unittest import mock

import pytest
from django.utils import timezone
from pydantic_ai import ModelRetry

from pi_dash.assistant.models import AssistantThread, AssistantTurn
from pi_dash.assistant.tools import issues, runs
from pi_dash.runner.models import AgentRun, AgentRunStatus, PinKind, Pod, Runner, RunnerStatus
from pi_dash.tests.contract.assistant.conftest import (
    ROLE_ADMIN,
    ROLE_MEMBER,
    _issue,
    fake_ctx,
    make_deps,
)

pytestmark = pytest.mark.django_db


@pytest.fixture(autouse=True)
def _no_activity():
    with mock.patch("pi_dash.bgtasks.issue_activities_task.issue_activity.delay"):
        yield


def _ctx(world, user, role=ROLE_ADMIN):
    thread = AssistantThread.objects.create(workspace=world.ws, user=user)
    AssistantTurn.objects.create(thread=thread)
    return fake_ctx(make_deps(user, world.ws, role, thread_id=thread.id))


def _pod(world):
    return Pod.default_for_project(world.proj_a)


def _runner(world, owner=None, name="gpu-box"):
    return Runner.objects.create(
        owner=owner or world.admin,
        workspace=world.ws,
        pod=_pod(world),
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _ident(issue):
    return f"{issue.project.identifier}-{issue.sequence_id}"


def test_create_issue_dedicates_to_runner_by_name(world):
    runner = _runner(world)
    ctx = _ctx(world, world.admin)
    res = issues.create_issue(ctx, project_id=str(world.proj_a.id), name="Needs GPU", assigned_runner="gpu-box")
    assert res["created"] is True
    assert res["assigned_runner_id"] == str(runner.id)
    assert res["assigned_pod_id"] == str(_pod(world).id)


def test_create_issue_with_pod_only(world):
    ctx = _ctx(world, world.admin)
    pod = _pod(world)
    pod_ref = pod.name.split("_", 1)[-1]
    res = issues.create_issue(
        ctx,
        project_id=str(world.proj_a.id),
        name="Pod work",
        assigned_pod=pod_ref,
    )
    assert res["created"] is True
    assert res["assigned_pod_id"] == str(pod.id)
    assert res["assigned_runner_id"] is None


def test_update_issue_sets_and_clears_runner(world):
    runner = _runner(world)
    target = _issue(world.proj_a, world.admin, "Target", seq=7, state=world.todo)
    ctx = _ctx(world, world.admin)
    res = issues.update_issue(ctx, issue_id=str(target.id), assigned_runner="gpu-box")
    assert res["updated"] is True
    assert res["assigned_runner_id"] == str(runner.id)

    cleared = issues.update_issue(ctx, issue_id=str(target.id), assigned_runner=None)
    assert cleared["updated"] is True
    assert cleared["assigned_runner_id"] is None


def test_update_issue_pod_only_clears_dedicated_runner(world):
    runner = _runner(world)
    issue = _issue(world.proj_a, world.admin, "Dedicated", seq=8, state=world.todo)
    issue.assigned_runner = runner
    issue.assigned_pod = _pod(world)
    issue.save(update_fields=["assigned_runner", "assigned_pod"])
    ctx = _ctx(world, world.admin)
    res = issues.update_issue(ctx, issue_id=str(issue.id), assigned_pod=_pod(world).name)
    assert res["updated"] is True
    assert res["assigned_runner_id"] is None


def test_update_issue_ecode_passes_through(world):
    # E7: the runner's owner must be the issue's creator or an assignee.
    # The member owns the runner (E1 passes) but is not on issue_a, so the
    # service refuses with runner_owner_not_on_issue.
    runner = _runner(world, owner=world.member)
    ctx = _ctx(world, world.member, role=ROLE_MEMBER)
    with pytest.raises(ModelRetry) as exc:
        issues.update_issue(ctx, issue_id=str(world.issue_a.id), assigned_runner=str(runner.id))
    assert "runner_owner_not_on_issue" in str(exc.value)


def test_update_issue_runner_not_found_passes_through(world):
    # E1: the member cannot use the admin's runner at all.
    runner = _runner(world)
    ctx = _ctx(world, world.member, role=ROLE_MEMBER)
    with pytest.raises(ModelRetry) as exc:
        issues.update_issue(ctx, issue_id=str(world.issue_a.id), assigned_runner=str(runner.id))
    assert "runner_not_found" in str(exc.value)


def test_dispatch_coding_run_rejects_unknown_pin(world):
    ctx = _ctx(world, world.admin)
    with pytest.raises(ModelRetry):
        runs.dispatch_coding_run(ctx, issue_id=str(world.issue_a.id), pin="bogus")


def test_dispatch_coding_run_threads_pod_once(world, mocker):
    ctx = _ctx(world, world.admin)
    fake_run = mock.Mock(id="11111111-1111-1111-1111-111111111111")
    outcome = mock.Mock(created_run=fake_run, reason="ok")
    mocked = mocker.patch(
        "pi_dash.orchestration.service.handle_issue_state_transition", return_value=outcome
    )
    res = runs.dispatch_coding_run(ctx, issue_id=str(world.issue_a.id), pin="pod_once")
    assert res["dispatched"] is True
    assert mocked.call_args.kwargs["pod_once"] is True

    mocked.reset_mock()
    runs.dispatch_coding_run(ctx, issue_id=str(world.issue_a.id))
    assert mocked.call_args.kwargs["pod_once"] is False


def test_get_run_status_carries_pin_and_wait(world):
    runner = _runner(world)
    issue = _issue(world.proj_a, world.admin, "Running", seq=10, state=world.todo)
    AgentRun.objects.create(
        workspace=world.ws,
        created_by=world.admin,
        pod=_pod(world),
        work_item=issue,
        status=AgentRunStatus.QUEUED,
        pinned_runner=runner,
        pin_kind=PinKind.DEDICATED,
    )
    ctx = _ctx(world, world.admin)
    res = runs.get_run_status(ctx, issue_id=str(issue.id))
    row = res["runs"][0]
    assert row["pinned_runner"] == str(runner.id)
    assert row["pinned_runner_name"] == "gpu-box"
    assert row["pin_kind"] == "dedicated"
    assert row["dispatch_wait"]["pin_kind"] == "dedicated"


def test_list_project_runners_with_assignable_for(world):
    from pi_dash.assistant.tools import projects as project_tools

    runner = _runner(world)
    ctx = _ctx(world, world.admin)
    rows = project_tools.list_project_runners(
        ctx,
        project_id=str(world.proj_a.id),
        assignable_for=str(world.issue_a.id),
    )
    row = next(r for r in rows if str(r["id"]) == str(runner.id))
    assert row["assignable"] is True
    assert row["not_assignable_code"] == ""
    assert row["is_busy"] is False
    assert row["queue_depth"] == 0


def test_get_issue_returns_assignment_and_wait(world):
    runner = _runner(world)
    target = _issue(world.proj_a, world.admin, "Waiting", seq=9, state=world.todo)
    target.assigned_runner = runner
    target.assigned_pod = _pod(world)
    target.save(update_fields=["assigned_runner", "assigned_pod"])
    ctx = _ctx(world, world.admin)
    data = issues.get_issue(ctx, issue_id=str(target.id))
    assert data["assigned_runner_id"] == str(runner.id)
    assert data["assigned_runner_detail"]["name"] == "gpu-box"
    assert data["dispatch_wait"] is None
