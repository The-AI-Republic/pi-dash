# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Automatic work, the waiting queue, and the sweep that bounds it.

The rule these tests pin: a closed laptop is *transient* and the work waits
visibly; anything that waiting cannot fix is *structural* and bounces loudly.
Getting this backwards either un-schedules people's issues every evening, or
queues runs forever against a machine that is never coming back.

Since PIDESKAPP-123 the waiting run is one strictly pinned to an issue's
``assigned_runner`` (``.ai_design/built_engine_as_runner/design.md`` §7-§9).
"""

from __future__ import annotations

from datetime import timedelta

import pytest
from django.test import override_settings
from django.utils import timezone

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.managed_runner.errors import ManagedRunnerReason
from pi_dash.managed_runner.tasks import expire_waiting_runs
from pi_dash.runner.models import AgentRun, AgentRunStatus, RunnerStatus

from .conftest import MANAGED_SETTINGS

pytestmark = pytest.mark.unit


def _assigned_issue(issue, runner):
    """Assign ``issue`` to ``runner``, created by the runner's owner.

    A queryset update, because ``BaseModel.save`` blanks ``created_by``
    outside a request and the usage rule reads it.
    """
    type(issue).objects.filter(pk=issue.pk).update(
        assigned_runner=runner, assigned_pod=runner.pod, created_by=runner.owner
    )
    issue.refresh_from_db()
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


def test_preflight_allows_a_closed_laptop(project, create_user, bundled_runner, issue_for_project):
    """Offline must pass the preflight so the run is created and waits.

    If this bounced, every issue whose owner shut their laptop would be moved
    back to Backlog and the ticker disarmed — losing the schedule, not just
    the run.
    """
    issue = _assigned_issue(issue_for_project, bundled_runner)
    bundled_runner.status = RunnerStatus.OFFLINE
    bundled_runner.save(update_fields=["status"])
    original_state = issue.state_id
    assert _preflight(issue, None, project) is True
    issue.refresh_from_db()
    assert issue.state_id == original_state


def test_preflight_bounces_a_pod_level_issue_with_only_a_pinned_only_engine(
    project, create_user, bundled_runner, issue_for_project
):
    assert _preflight(issue_for_project, create_user, project) is False


def test_preflight_bounces_a_tick_on_someone_elses_engine(
    project, create_user, create_user2, issue_for_project
):
    """The engine's owner is neither the issue's creator nor an assignee: the
    usage rule would never admit the tick, so bounce instead of queueing."""
    from .conftest import make_runner
    from pi_dash.runner.models import RunnerProvisioning

    engine = make_runner(owner=create_user2, project=project, provisioning=RunnerProvisioning.DESKTOP_BUNDLED)
    issue = _assigned_issue(issue_for_project, engine)
    type(issue).objects.filter(pk=issue.pk).update(created_by=create_user)
    issue.refresh_from_db()
    assert _preflight(issue, None, project) is False


def test_local_issues_are_unaffected(project, create_user, manual_runner, issue_for_project):
    """Regression guard: own-agent runners keep their eligibility rules."""
    assert _preflight(issue_for_project, create_user, project) is True


# --------------------------------------------------------------------------
# Creator resolution
# --------------------------------------------------------------------------


def test_tick_on_an_assigned_issue_runs_as_the_runner_owner(
    project, create_user, bundled_runner, issue_for_project
):
    """§7: for ticks and schedulers the run's principal is the runner's owner
    when the usage rule allows it."""
    from pi_dash.orchestration import scheduling

    issue = _assigned_issue(issue_for_project, bundled_runner)
    resolved = scheduling._resolve_creator_for_trigger(issue, triggered_by=scheduling.TRIGGER_TICK)
    assert resolved == create_user


def test_explicit_actor_still_wins(project, create_user, create_user2, bundled_runner, issue_for_project):
    from pi_dash.orchestration import scheduling

    issue = _assigned_issue(issue_for_project, bundled_runner)
    resolved = scheduling._resolve_creator_for_trigger(
        issue, triggered_by=scheduling.TRIGGER_RUN_AI, actor=create_user2
    )
    assert resolved == create_user2


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
        pin_strict=True,
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
    # The message names the machine (the daemon runs without the app, so
    # "open the desktop app" is no longer the fix) and what to do next.
    assert run.error.startswith("rich-laptop is offline")
    assert "run the issue again" in run.error


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
