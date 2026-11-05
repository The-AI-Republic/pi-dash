# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Run creation and matching for built-in engine runners.

Since PIDESKAPP-123 (``.ai_design/built_engine_as_runner/design.md`` §6-§8)
the engine is a runner like any other: ``managed_runner`` is a legacy executor
value, an issue reaches one specific engine through ``Issue.assigned_runner``
(a *strict* pin), and an engine takes pod work only once its owner makes it a
pod member. Two invariants still carry most of the weight here:

1. a strictly pinned run is created *for one runner* and can never drift to
   another;
2. a ``pinned_only`` engine never receives work that was not strictly pinned
   to it, so one person's laptop cannot become the team's build server.
"""

from __future__ import annotations

import pytest
from django.db import transaction
from django.test import override_settings

from pi_dash.cloud_agent.creation import execution_fields
from pi_dash.cloud_agent.policy import resolve_executor_kind
from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    RunnerPodDispatch,
    RunnerProvisioning,
    RunnerStatus,
)
from pi_dash.runner.services import matcher
from pi_dash.runner.services.dispatch_rules import (
    ASSIGNED_RUNNER_NOT_OWNER,
    ASSIGNED_RUNNER_OFFLINE,
    WAITING_FOR_RUNNER,
    AssignedRunnerUnavailable,
)

from .conftest import MANAGED_SETTINGS, make_runner

pytestmark = pytest.mark.unit


def _assign(issue, runner):
    """Assign ``issue`` to ``runner``, created by the runner's owner.

    A queryset update, because ``BaseModel.save`` blanks ``created_by``
    outside a request and the usage rule reads it.
    """
    type(issue).objects.filter(pk=issue.pk).update(
        assigned_runner=runner, assigned_pod=runner.pod, created_by=runner.owner
    )
    issue.refresh_from_db()
    return issue


def _fields(project, user, issue=None, **kw):
    return execution_fields(
        project=project,
        run_kind="issue",
        has_issue=True,
        requested=AgentExecutorKind.MANAGED_RUNNER,
        actor=user,
        issue=issue,
        **kw,
    )


# --------------------------------------------------------------------------
# Executor resolution
# --------------------------------------------------------------------------


@pytest.mark.parametrize("enabled", [True, False])
def test_resolve_maps_legacy_managed_to_local(project, enabled):
    with override_settings(MANAGED_RUNNER_ENABLED=enabled):
        assert (
            resolve_executor_kind(project=project, requested=AgentExecutorKind.MANAGED_RUNNER)
            == AgentExecutorKind.LOCAL_RUNNER
        )


def test_engine_runner_starts_pinned_only(bundled_runner, manual_runner):
    assert bundled_runner.pod_dispatch == RunnerPodDispatch.PINNED_ONLY
    assert manual_runner.pod_dispatch == RunnerPodDispatch.POD_MEMBER


@pytest.mark.parametrize("online", [True, False])
@override_settings(**MANAGED_SETTINGS)
def test_project_move_handoff_keeps_strict_pin_in_destination(
    project, create_user, bundled_runner, issue_for_project, online
):
    from unittest.mock import patch

    from pi_dash.orchestration.service import _create_project_move_handoff_run

    if not online:
        bundled_runner.status = RunnerStatus.OFFLINE
        bundled_runner.save(update_fields=["status"])
    issue = _assign(issue_for_project, bundled_runner)
    parent = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=bundled_runner.pod,
        work_item=issue,
        executor_kind=AgentExecutorKind.LOCAL_RUNNER,
        status=AgentRunStatus.CANCELLED,
        prompt="prior run",
    )
    with patch("pi_dash.orchestration.service.build_first_turn", return_value="moved task"), patch(
        "pi_dash.cloud_agent.creation.dispatch_after_commit"
    ):
        run = _create_project_move_handoff_run(issue=issue, parent=parent, pod=bundled_runner.pod)
    assert run.executor_kind == AgentExecutorKind.LOCAL_RUNNER
    assert run.pinned_runner_id == bundled_runner.id
    assert run.pin_strict is True
    assert run.status == AgentRunStatus.QUEUED
    if not online:
        assert run.error_code == WAITING_FOR_RUNNER
        bundled_runner.status = RunnerStatus.ONLINE
        bundled_runner.save(update_fields=["status"])
    with transaction.atomic():
        assert matcher.next_for_runner(bundled_runner).id == run.id


# --------------------------------------------------------------------------
# Creation
# --------------------------------------------------------------------------


def test_pod_level_issue_gets_no_pin(project, create_user, issue_for_project, bundled_runner):
    fields = _fields(project, create_user, issue_for_project)
    assert fields == {"executor_kind": AgentExecutorKind.LOCAL_RUNNER, "tool_plan": {}}


def test_creation_pins_strictly_to_the_assigned_runner(project, create_user, issue_for_project, bundled_runner):
    issue = _assign(issue_for_project, bundled_runner)
    fields = _fields(project, create_user, issue)
    assert fields["executor_kind"] == AgentExecutorKind.LOCAL_RUNNER
    assert fields["pinned_runner"] == bundled_runner
    assert fields["pin_strict"] is True
    assert "error_details" not in fields


def test_teammates_click_on_an_engine_assigned_issue_is_refused(
    project, create_user, create_user2, issue_for_project, bundled_runner
):
    """PO-10: the issue names one person's engine; a teammate's Run is refused
    rather than queued for a runner that would never take it."""
    issue = _assign(issue_for_project, bundled_runner)
    with pytest.raises(AssignedRunnerUnavailable) as exc:
        _fields(project, create_user2, issue)
    assert exc.value.code == ASSIGNED_RUNNER_NOT_OWNER


def test_teammates_click_is_admitted_with_the_owners_opt_in(
    project, create_user, create_user2, issue_for_project, bundled_runner, desktop_machine
):
    desktop_machine.engine_team_runs = True
    desktop_machine.save(update_fields=["engine_team_runs"])
    issue = _assign(issue_for_project, bundled_runner)
    fields = _fields(project, create_user2, issue)
    assert fields["pinned_runner"] == bundled_runner


def test_person_started_run_is_refused_when_runner_offline(project, create_user, issue_for_project, bundled_runner):
    """PO-5: refused at creation, naming the machine."""
    bundled_runner.status = RunnerStatus.OFFLINE
    bundled_runner.save(update_fields=["status"])
    issue = _assign(issue_for_project, bundled_runner)
    with pytest.raises(AssignedRunnerUnavailable) as exc:
        _fields(project, create_user, issue, automatic=False)
    assert exc.value.code == ASSIGNED_RUNNER_OFFLINE
    assert str(exc.value) == "rich-laptop is offline"


def test_person_started_run_queues_behind_a_busy_runner(project, create_user, issue_for_project, bundled_runner):
    bundled_runner.status = RunnerStatus.BUSY
    bundled_runner.save(update_fields=["status"])
    issue = _assign(issue_for_project, bundled_runner)
    fields = _fields(project, create_user, issue, automatic=False)
    assert fields["pinned_runner"] == bundled_runner
    assert "error_details" not in fields


def test_paused_engine_counts_as_offline(project, create_user, issue_for_project, bundled_runner, desktop_machine):
    desktop_machine.engine_paused_by_owner = True
    desktop_machine.save(update_fields=["engine_paused_by_owner"])
    issue = _assign(issue_for_project, bundled_runner)
    with pytest.raises(AssignedRunnerUnavailable) as exc:
        _fields(project, create_user, issue, automatic=False)
    assert exc.value.code == ASSIGNED_RUNNER_OFFLINE


def test_automatic_run_queues_visibly_when_runner_offline(project, create_user, issue_for_project, bundled_runner):
    """A tick firing while the lid is shut waits, not bounces."""
    bundled_runner.status = RunnerStatus.OFFLINE
    bundled_runner.save(update_fields=["status"])
    issue = _assign(issue_for_project, bundled_runner)
    fields = _fields(project, create_user, issue, automatic=True)
    assert fields["pinned_runner"] == bundled_runner
    assert fields["pin_strict"] is True
    assert fields["error_details"] == {"code": WAITING_FOR_RUNNER}


def test_revoked_assigned_runner_never_pins(project, create_user, issue_for_project, bundled_runner):
    issue = _assign(issue_for_project, bundled_runner)
    from pi_dash.runner.models import Runner

    Runner.objects.filter(pk=bundled_runner.pk).update(status=RunnerStatus.REVOKED)
    assert "pinned_runner" not in _fields(project, create_user, issue)


# --------------------------------------------------------------------------
# Persistence: the check constraint still admits legacy rows
# --------------------------------------------------------------------------


@override_settings(**MANAGED_SETTINGS)
def test_legacy_managed_run_row_is_insertable_with_a_pinned_runner(project, create_user, bundled_runner):
    """``agent_run_cloud_has_no_local_assignment`` is unchanged: history keeps
    its ``managed_runner`` rows."""
    run = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=project.pods.get(is_default=True),
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        pinned_runner=bundled_runner,
        pin_strict=True,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    run.refresh_from_db()
    assert run.executor_kind == AgentExecutorKind.MANAGED_RUNNER
    assert run.pinned_runner_id == bundled_runner.id


# --------------------------------------------------------------------------
# Matching
# --------------------------------------------------------------------------


@pytest.mark.parametrize("drain", ["runner", "pod"])
def test_pinned_only_engine_keeps_unpinned_work_off_desktop(project, create_user, bundled_runner, drain):
    from unittest.mock import patch

    pod = project.pods.get(is_default=True)
    run = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=pod,
        executor_kind=AgentExecutorKind.LOCAL_RUNNER,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        if drain == "runner":
            assert matcher.drain_for_runner(bundled_runner) is False
        else:
            assert matcher.drain_pod(pod) == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.runner_id is None


def test_pod_member_engine_takes_its_owners_unpinned_work(project, create_user, bundled_runner):
    from unittest.mock import patch

    bundled_runner.pod_dispatch = RunnerPodDispatch.POD_MEMBER
    bundled_runner.save(update_fields=["pod_dispatch"])
    run = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=bundled_runner.pod,
        executor_kind=AgentExecutorKind.LOCAL_RUNNER,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        assert matcher.drain_for_runner(bundled_runner) is True
    run.refresh_from_db()
    assert run.runner_id == bundled_runner.id


def test_legacy_managed_pin_is_still_delivered(project, create_user, bundled_runner):
    from unittest.mock import patch

    run = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=bundled_runner.pod,
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        pinned_runner=bundled_runner,
        pin_strict=True,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        assert matcher.drain_for_runner(bundled_runner) is True
    run.refresh_from_db()
    assert run.runner_id == bundled_runner.id
    assert run.status == AgentRunStatus.ASSIGNED


def test_strict_pin_to_engine_never_reaches_manual_runner(project, create_user, bundled_runner, manual_runner):
    AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=manual_runner.pod,
        executor_kind=AgentExecutorKind.LOCAL_RUNNER,
        pinned_runner=bundled_runner,
        pin_strict=True,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    with transaction.atomic():
        assert matcher.next_for_runner(manual_runner) is None
        assert matcher.next_queued_run_for_pod(manual_runner.pod) is None


@override_settings(**MANAGED_SETTINGS)
def test_pinned_only_engine_is_not_selected_for_pod_work(project, create_user, bundled_runner):
    pod = project.pods.get(is_default=True)
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None
    manual = make_runner(owner=create_user, project=project, provisioning=RunnerProvisioning.MANUAL)
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) == manual


def test_preflight_counts_an_assigned_engine_even_when_offline(
    project, create_user, bundled_runner, issue_for_project
):
    """Offline is not structural: the run is created and waits (§9)."""
    issue = _assign(issue_for_project, bundled_runner)
    pod = project.pods.get(is_default=True)
    bundled_runner.status = RunnerStatus.OFFLINE
    bundled_runner.save(update_fields=["status"])
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, None, human=False) is True

    # Revoked *is* structural — nothing can ever serve it.
    bundled_runner.status = RunnerStatus.REVOKED
    bundled_runner.save(update_fields=["status"])
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, None, human=False) is False


def test_pod_level_preflight_ignores_pinned_only_engines(project, create_user, bundled_runner, issue_for_project):
    """A pod-level issue is not serviceable just because its creator has an
    engine that is not a pod member."""
    pod = project.pods.get(is_default=True)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue_for_project, create_user.id) is False
    bundled_runner.pod_dispatch = RunnerPodDispatch.POD_MEMBER
    bundled_runner.save(update_fields=["pod_dispatch"])
    assert matcher.pod_has_runner_for_issue_principal(pod, issue_for_project, create_user.id) is True
