# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``pidash_create_project_issue`` dedicated-runner inputs (HA-5, §14.6).

The in-run create tool accepts ``assigned_runner``/``assigned_pod`` through
the shared service; the acting user is the run's creator (E1/E7 apply to
them), and E-codes surface as tool errors.
"""

from unittest.mock import patch

import pytest
from django.test import override_settings
from django.utils import timezone

from pi_dash.cloud_agent.policy import build_tool_plan
from pi_dash.cloud_agent.tools import build_tools
from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.db.models import ProjectMember, Scheduler, SchedulerBinding, State
from pi_dash.runner.models import AgentRun, Pod, Runner, RunnerStatus


@pytest.fixture(autouse=True)
def _no_activity():
    with patch("pi_dash.bgtasks.issue_activities_task.issue_activity.delay"):
        yield


@pytest.fixture
def scheduler_run(project, workspace, create_user):
    ProjectMember.objects.get_or_create(
        workspace=workspace, project=project, member=create_user, defaults={"role": 20}
    )
    State.objects.create(name="Backlog", group="backlog", project=project, sequence=1.0, default=True)
    from crum import impersonate

    with impersonate(create_user):
        scheduler = Scheduler.objects.create(
            workspace=workspace, slug="nightly", name="Nightly", prompt="Audit.", color="#10b981"
        )
        binding = SchedulerBinding.objects.create(
            scheduler=scheduler,
            project=project,
            workspace=workspace,
            dtstart=timezone.now(),
            rrule="FREQ=DAILY",
            tzid="UTC",
            enabled=True,
            actor=create_user,
        )
    return AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=Pod.default_for_project(project),
        scheduler_binding=binding,
        executor_kind=AgentExecutorKind.CLOUD_AGENT,
        tool_plan=build_tool_plan(run_kind="scheduler", has_issue=False),
        prompt="Bound task",
    )


def _tools(run):
    # The plan must be built under the enabled settings (fixtures run
    # outside @override_settings), so rebuild it here.
    plan = build_tool_plan(run_kind="scheduler", has_issue=False)
    run.tool_plan = plan
    run.save(update_fields=["tool_plan"])
    return {tool.__name__: tool for tool in build_tools(run.id, plan["tools"])}


def _runner(create_user, workspace, project, name="gpu-box"):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=Pod.default_for_project(project),
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.mark.unit
@override_settings(CLOUD_AGENT_ENABLED=True, CLOUD_AGENT_WRITES_ENABLED=True)
def test_create_issue_with_dedicated_runner(scheduler_run, project, create_user, workspace):
    runner = _runner(create_user, workspace, project)
    tools = _tools(scheduler_run)
    res = tools["pidash_create_project_issue"]("Needs GPU", assigned_runner="gpu-box")
    assert res["created"] is True
    assert res["assigned_runner_id"] == str(runner.id)
    assert res["assigned_pod_id"] == str(Pod.default_for_project(project).id)


@pytest.mark.unit
@override_settings(CLOUD_AGENT_ENABLED=True, CLOUD_AGENT_WRITES_ENABLED=True)
def test_create_issue_unknown_runner_is_tool_error(scheduler_run):
    tools = _tools(scheduler_run)
    with pytest.raises(ValueError, match="not found"):
        tools["pidash_create_project_issue"]("Needs GPU", assigned_runner="nope")
