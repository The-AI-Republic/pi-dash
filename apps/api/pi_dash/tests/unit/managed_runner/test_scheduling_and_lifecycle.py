# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Automatic work, the waiting queue, and the sweep that bounds it.

The rule these tests pin: a closed laptop is *transient* and the work waits
visibly; anything that waiting cannot fix is *structural* and bounces loudly.
Getting this backwards either un-schedules people's issues every evening, or
queues runs forever against a machine that is never coming back.
"""

from __future__ import annotations

from datetime import timedelta

import pytest
from django.test import override_settings
from django.utils import timezone

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.managed_runner.errors import ManagedRunnerReason
from pi_dash.managed_runner.tasks import expire_waiting_runs
from pi_dash.runner.models import AgentRun, AgentRunStatus, RunnerPodDispatch, RunnerStatus

from .conftest import MANAGED_SETTINGS

pytestmark = pytest.mark.unit


def _managed_issue(issue, project):
    """An issue still carrying the legacy ``managed_runner`` value (one the
    data migration has not reached, or written by an old path)."""
    issue.agent_executor = AgentExecutorKind.MANAGED_RUNNER
    issue.save(update_fields=["agent_executor"])
    return issue


def _preflight(issue, user, project, triggered_by="tick"):
    from pi_dash.orchestration.scheduling import preflight_eligibility_or_bounce

    return preflight_eligibility_or_bounce(
        issue,
        run_creator=user,
        pod=project.pods.get(is_default=True),
        triggered_by=triggered_by,
    )


# --------------------------------------------------------------------------
# Preflight: structural vs transient
# --------------------------------------------------------------------------


def test_preflight_allows_a_closed_laptop_in_the_pod(project, create_user, bundled_runner, issue_for_project):
    """A legacy ``managed_runner`` issue is a pod issue now (§13). With the
    owner's engine a pod member, offline must pass the preflight so the run
    is created and waits in the pod.

    If this bounced, every issue whose owner shut their laptop would be moved
    back to Backlog and the ticker disarmed — losing the schedule, not just
    the run.
    """
    issue = _managed_issue(issue_for_project, project)
    bundled_runner.pod_dispatch = RunnerPodDispatch.POD_MEMBER
    bundled_runner.status = RunnerStatus.OFFLINE
    bundled_runner.save(update_fields=["pod_dispatch", "status"])
    original_state = issue.state_id
    assert _preflight(issue, create_user, project) is True
    issue.refresh_from_db()
    assert issue.state_id == original_state


def test_preflight_bounces_when_nothing_is_enrolled(project, create_user, issue_for_project):
    issue = _managed_issue(issue_for_project, project)
    assert _preflight(issue, create_user, project) is False


def test_preflight_bounces_when_the_only_engine_has_pod_work_off(
    project, create_user, bundled_runner, issue_for_project
):
    """Scenario 18 and 20: a ``pinned_only`` engine never takes new issue
    work, so counting it would leave the issue queued for ever."""
    issue = _managed_issue(issue_for_project, project)
    assert bundled_runner.pod_dispatch == RunnerPodDispatch.PINNED_ONLY
    assert _preflight(issue, create_user, project) is False


def test_preflight_bounces_without_a_creator(project, bundled_runner, issue_for_project):
    issue = _managed_issue(issue_for_project, project)
    assert _preflight(issue, None, project) is False


def test_local_issues_are_unaffected(project, create_user, manual_runner, issue_for_project):
    """Regression guard: own-agent runners keep their eligibility rules."""
    assert _preflight(issue_for_project, create_user, project) is True


# --------------------------------------------------------------------------
# Creator resolution
# --------------------------------------------------------------------------


def test_creator_resolution_for_a_legacy_managed_issue_is_the_actor(project, create_user, issue_for_project):
    """A legacy managed issue resolves like any runner issue: no desktop is
    required of the principal any more (the pod decides who runs it)."""
    from pi_dash.orchestration import scheduling

    issue = _managed_issue(issue_for_project, project)
    resolved = scheduling._resolve_creator_for_trigger(
        issue, triggered_by=scheduling.TRIGGER_RUN_AI, actor=create_user
    )
    assert resolved == create_user


# --------------------------------------------------------------------------
# The waiting queue and its bound
# --------------------------------------------------------------------------


def _waiting_run(project, user, runner, *, age=timedelta(0)):
    run = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=user,
        pod=project.pods.get(is_default=True),
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        pinned_runner=runner,
        status=AgentRunStatus.QUEUED,
        error_details={"code": ManagedRunnerReason.NOT_CONNECTED},
        prompt="",
    )
    if age:
        AgentRun.objects.filter(pk=run.pk).update(created_at=timezone.now() - age)
    return run


@override_settings(**MANAGED_SETTINGS, MANAGED_RUNNER_QUEUED_MAX_AGE_SECS=3600)
def test_sweep_leaves_a_recent_wait_alone(project, create_user, bundled_runner):
    """A laptop closed over lunch must not lose its queued work."""
    run = _waiting_run(project, create_user, bundled_runner)
    assert expire_waiting_runs() == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED


@pytest.mark.parametrize("new_status", [AgentRunStatus.ASSIGNED, AgentRunStatus.RUNNING, AgentRunStatus.CANCELLED])
@override_settings(**MANAGED_SETTINGS, MANAGED_RUNNER_QUEUED_MAX_AGE_SECS=3600)
def test_sweep_rechecks_status_after_its_scan(project, create_user, bundled_runner, monkeypatch, new_status):
    from pi_dash.runner.services.agent_run_finalization import finalize_agent_run

    run = _waiting_run(project, create_user, bundled_runner, age=timedelta(hours=2))

    def transition_before_lock(run_id, *args, **kwargs):
        AgentRun.objects.filter(pk=run_id).update(status=new_status, runner=bundled_runner)
        return finalize_agent_run(run_id, *args, **kwargs)

    monkeypatch.setattr("pi_dash.managed_runner.tasks.finalize_agent_run", transition_before_lock)
    assert expire_waiting_runs() == 0
    run.refresh_from_db()
    assert run.status == new_status


@override_settings(**MANAGED_SETTINGS, MANAGED_RUNNER_QUEUED_MAX_AGE_SECS=3600)
def test_sweep_fails_a_wait_that_outlived_the_bound(project, create_user, bundled_runner):
    run = _waiting_run(project, create_user, bundled_runner, age=timedelta(hours=2))
    assert expire_waiting_runs() == 1
    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert run.error_code == ManagedRunnerReason.NOT_CONNECTED
    # The message must tell the user what to do, not just what happened.
    assert "desktop" in run.error.lower()


@override_settings(**MANAGED_SETTINGS, MANAGED_RUNNER_QUEUED_MAX_AGE_SECS=3600)
def test_sweep_ignores_other_executors_and_non_queued_rows(project, create_user, bundled_runner):
    """The sweep is scoped to managed *queued* work: a running managed run is
    the heartbeat reaper's business, and cloud/local rows have their own."""
    old = timezone.now() - timedelta(hours=2)

    running = _waiting_run(project, create_user, bundled_runner, age=timedelta(hours=2))
    AgentRun.objects.filter(pk=running.pk).update(status=AgentRunStatus.RUNNING)

    local = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=project.pods.get(is_default=True),
        executor_kind=AgentExecutorKind.LOCAL_RUNNER,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    AgentRun.objects.filter(pk=local.pk).update(created_at=old)

    assert expire_waiting_runs() == 0
    running.refresh_from_db()
    local.refresh_from_db()
    assert running.status == AgentRunStatus.RUNNING
    assert local.status == AgentRunStatus.QUEUED
