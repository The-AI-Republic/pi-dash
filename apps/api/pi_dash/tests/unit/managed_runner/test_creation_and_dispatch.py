# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Run creation, pinning, and the matcher's treatment of bundled runners.

Two invariants carry most of the weight here:

1. a managed run is created *for one machine* and can never drift to another;
2. a bundled runner never receives work that was not pinned to it, so one
   person's laptop cannot become the team's build server.
"""

from __future__ import annotations

import pytest
from django.test import override_settings

from pi_dash.cloud_agent.creation import execution_fields
from pi_dash.cloud_agent.policy import resolve_executor_kind
from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.managed_runner.errors import ManagedRunnerReason, ManagedRunnerUnavailable
from pi_dash.runner.models import AgentRun, AgentRunStatus, RunnerProvisioning, RunnerStatus
from pi_dash.runner.services import matcher

from .conftest import MANAGED_SETTINGS, make_runner

pytestmark = pytest.mark.unit


def _fields(project, user, **kw):
    return execution_fields(
        project=project,
        run_kind="issue",
        has_issue=True,
        requested=AgentExecutorKind.MANAGED_RUNNER,
        actor=user,
        **kw,
    )


# --------------------------------------------------------------------------
# Executor resolution
# --------------------------------------------------------------------------


@pytest.mark.parametrize("online", [True, False])
@override_settings(**MANAGED_SETTINGS)
def test_project_move_preserves_destination_managed_pin(
    project, create_user, bundled_runner, issue_for_project, openhub_lane, online
):
    from unittest.mock import patch
    from django.db import transaction
    from pi_dash.orchestration.service import _create_project_move_handoff_run

    if not online:
        bundled_runner.status = RunnerStatus.OFFLINE
        bundled_runner.save(update_fields=["status"])
    issue = issue_for_project
    issue.agent_executor = AgentExecutorKind.MANAGED_RUNNER
    issue.save(update_fields=["agent_executor"])
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
    assert run.executor_kind == AgentExecutorKind.MANAGED_RUNNER
    assert run.pinned_runner_id == bundled_runner.id
    assert run.status == AgentRunStatus.QUEUED
    if not online:
        assert run.error_code == ManagedRunnerReason.NOT_CONNECTED
        bundled_runner.status = RunnerStatus.ONLINE
        bundled_runner.save(update_fields=["status"])
    with transaction.atomic():
        assert matcher.next_for_runner(bundled_runner).id == run.id


@override_settings(**MANAGED_SETTINGS)
def test_resolve_accepts_managed_when_enabled(project):
    assert (
        resolve_executor_kind(project=project, requested=AgentExecutorKind.MANAGED_RUNNER)
        == AgentExecutorKind.MANAGED_RUNNER
    )


@override_settings(MANAGED_RUNNER_ENABLED=False)
def test_resolve_refuses_managed_when_disabled(project):
    with pytest.raises(ManagedRunnerUnavailable) as exc:
        resolve_executor_kind(project=project, requested=AgentExecutorKind.MANAGED_RUNNER)
    assert exc.value.code == ManagedRunnerReason.DISABLED


# --------------------------------------------------------------------------
# Creation
# --------------------------------------------------------------------------


@override_settings(**MANAGED_SETTINGS)
def test_creation_pins_to_the_creators_own_runner(project, create_user, openhub_lane, bundled_runner):
    fields = _fields(project, create_user)
    assert fields["executor_kind"] == AgentExecutorKind.MANAGED_RUNNER
    assert fields["pinned_runner"] == bundled_runner
    assert fields["tool_plan"] == {}
    # No waiting marker on the happy path.
    assert "error_code" not in fields


@override_settings(**MANAGED_SETTINGS)
def test_creation_never_pins_to_a_teammates_desktop(
    project, create_user, create_user2, openhub_lane, bundled_runner
):
    """``create_user2``'s run must not land on ``create_user``'s laptop, even
    though both runners sit on the same pod."""
    with pytest.raises(ManagedRunnerUnavailable) as exc:
        _fields(project, create_user2)
    assert exc.value.code == ManagedRunnerReason.NO_RUNNER_FOR_PROJECT


@override_settings(**MANAGED_SETTINGS)
def test_user_triggered_run_is_refused_when_desktop_offline(
    project, create_user, openhub_lane, bundled_runner
):
    """The click came from the desktop, so "your desktop is closed" is an
    immediate, actionable error rather than a row that waits."""
    bundled_runner.status = RunnerStatus.OFFLINE
    bundled_runner.save(update_fields=["status"])
    with pytest.raises(ManagedRunnerUnavailable) as exc:
        _fields(project, create_user, automatic=False)
    assert exc.value.code == ManagedRunnerReason.NOT_CONNECTED
    assert "desktop app" in str(exc.value)


@override_settings(**MANAGED_SETTINGS)
def test_automatic_run_queues_visibly_when_desktop_offline(
    project, create_user, openhub_lane, bundled_runner
):
    """A ticker firing while the lid is shut should wait, not bounce: the work
    is still valid and the machine will be back."""
    bundled_runner.status = RunnerStatus.OFFLINE
    bundled_runner.save(update_fields=["status"])
    fields = _fields(project, create_user, automatic=True)
    assert fields["pinned_runner"] == bundled_runner
    assert fields["error_code"] == ManagedRunnerReason.NOT_CONNECTED


@override_settings(**MANAGED_SETTINGS)
def test_automatic_run_still_refused_for_structural_failures(project, create_user, byok_lane):
    """Waiting cannot fix an unsupported provider, so automatic runs are
    refused for it exactly like user-triggered ones."""
    with pytest.raises(ManagedRunnerUnavailable) as exc:
        _fields(project, create_user, automatic=True)
    assert exc.value.code == ManagedRunnerReason.BYOK_UNSUPPORTED


@override_settings(**MANAGED_SETTINGS)
def test_refusal_details_are_actionable(project, create_user, byok_lane):
    with pytest.raises(ManagedRunnerUnavailable) as exc:
        _fields(project, create_user)
    detail = str(exc.value)
    assert "OpenHub" in detail
    # And it must reassure the user their key still works elsewhere.
    assert "Cloud Agent" in detail


# --------------------------------------------------------------------------
# Persistence: the check constraint must admit the new kind
# --------------------------------------------------------------------------


@override_settings(**MANAGED_SETTINGS)
def test_managed_run_row_is_insertable_with_a_pinned_runner(
    project, create_user, openhub_lane, bundled_runner
):
    """Regression for the ``agent_run_cloud_has_no_local_assignment`` check:
    before it was widened, a managed row satisfied neither branch and every
    insert raised IntegrityError."""
    run = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=project.pods.get(is_default=True),
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        pinned_runner=bundled_runner,
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
def test_actual_dispatch_keeps_unpinned_local_work_off_desktop(project, create_user, bundled_runner, drain):
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


def test_actual_dispatch_delivers_only_managed_pin_to_desktop(project, create_user, bundled_runner):
    from unittest.mock import patch

    run = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=bundled_runner.pod,
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        pinned_runner=bundled_runner,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        assert matcher.drain_for_runner(bundled_runner) is True
    run.refresh_from_db()
    assert run.runner_id == bundled_runner.id
    assert run.status == AgentRunStatus.ASSIGNED


def test_unpinned_managed_run_cannot_fall_back_to_manual_runner(project, create_user, manual_runner):
    AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=manual_runner.pod,
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    assert matcher.next_for_runner(manual_runner) is None


@override_settings(**MANAGED_SETTINGS)
def test_bundled_runner_never_takes_unpinned_pod_work(project, create_user, bundled_runner):
    """``select_runner_in_pod`` powers unpinned local dispatch; a bundled
    runner appearing there would hand a teammate's issue to someone's laptop."""
    pod = project.pods.get(is_default=True)
    from django.db import transaction

    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None

    manual = make_runner(owner=create_user, project=project, provisioning=RunnerProvisioning.MANUAL)
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) == manual


@override_settings(**MANAGED_SETTINGS)
def test_pinned_managed_run_is_not_offered_by_drain_pod(
    project, create_user, openhub_lane, bundled_runner
):
    """Pinned runs are delivered only through ``next_for_runner`` on the
    pinned machine's heartbeat — never by the pod-wide drain."""
    pod = project.pods.get(is_default=True)
    AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=pod,
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        pinned_runner=bundled_runner,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    assert matcher.next_queued_run_for_pod(pod) is None
    assert matcher.next_for_runner(bundled_runner) is not None


@override_settings(**MANAGED_SETTINGS)
def test_preflight_is_structural_for_managed_issues(
    project, create_user, openhub_lane, bundled_runner, issue_for_project
):
    """Offline is not a structural failure: the preflight must still say yes
    so the run is created and waits (§8.5), rather than bouncing the issue."""
    issue_for_project.agent_executor = AgentExecutorKind.MANAGED_RUNNER
    issue_for_project.save(update_fields=["agent_executor"])
    pod = project.pods.get(is_default=True)

    bundled_runner.status = RunnerStatus.OFFLINE
    bundled_runner.save(update_fields=["status"])
    assert matcher.pod_has_runner_for_issue_principal(pod, issue_for_project, create_user.id) is True

    # Revoked *is* structural — nothing can ever serve it.
    bundled_runner.status = RunnerStatus.REVOKED
    bundled_runner.save(update_fields=["status"])
    assert matcher.pod_has_runner_for_issue_principal(pod, issue_for_project, create_user.id) is False


@override_settings(**MANAGED_SETTINGS)
def test_local_preflight_ignores_bundled_runners(project, create_user, bundled_runner, issue_for_project):
    """A local-runner issue must not be considered serviceable just because the
    creator has a desktop app open."""
    pod = project.pods.get(is_default=True)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue_for_project, create_user.id) is False
    make_runner(owner=create_user, project=project, provisioning=RunnerProvisioning.MANUAL)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue_for_project, create_user.id) is True
