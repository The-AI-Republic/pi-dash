# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Availability policy: who may run on their own desktop, and why not.

The reason a user sees in the picker, the reason the API refuses with, and the
reason the desktop reports must all come from one place — otherwise a user is
told two different things about one situation. These tests pin the precedence
order that makes that true.
"""

from __future__ import annotations

from datetime import timedelta

import pytest
from django.test import override_settings
from django.utils import timezone

from pi_dash.core.agent_execution import AgentExecutorKind, agent_executor_options
from pi_dash.managed_runner.errors import ManagedRunnerReason
from pi_dash.managed_runner.policy import (
    enrolled_managed_runners,
    managed_runner_availability,
    online_managed_runner,
)
from pi_dash.runner.models import Runner, RunnerProvisioning, RunnerStatus

from .conftest import MANAGED_SETTINGS, make_runner

pytestmark = pytest.mark.unit


@override_settings(**MANAGED_SETTINGS)
def test_available_when_enabled_configured_and_online(project, create_user, openhub_lane, bundled_runner):
    available, reason = managed_runner_availability(project, create_user)
    assert available is True
    assert reason == ""
    assert online_managed_runner(project, create_user) == bundled_runner


@override_settings(MANAGED_RUNNER_ENABLED=False)
def test_kill_switch_wins_over_everything(project, create_user, openhub_lane, bundled_runner):
    """The operator switch is checked first so a disabled instance never leaks
    a per-user reason that implies the feature exists for them."""
    available, reason = managed_runner_availability(project, create_user)
    assert available is False
    assert reason == ManagedRunnerReason.DISABLED


@override_settings(**MANAGED_SETTINGS)
def test_no_viewer_is_not_connected(project, openhub_lane):
    """Availability is viewer-scoped; without a viewer there is no desktop."""
    available, reason = managed_runner_availability(project, None)
    assert available is False
    assert reason == ManagedRunnerReason.NOT_CONNECTED


@override_settings(**MANAGED_SETTINGS)
def test_provider_gate_precedes_runner_gate(project, create_user, byok_lane, bundled_runner):
    """A BYOK user with a perfectly healthy desktop still cannot run.

    Precedence matters here: reporting "desktop not connected" for a connected
    desktop would send the user to fix the wrong thing.
    """
    available, reason = managed_runner_availability(project, create_user)
    assert available is False
    assert reason == ManagedRunnerReason.BYOK_UNSUPPORTED


@override_settings(**MANAGED_SETTINGS)
def test_no_runner_for_project_is_distinct_from_offline(project, create_user, openhub_lane):
    """Nothing enrolled yet is a different situation from a closed laptop.

    The desktop fixes the first silently by enrolling; only the second is
    worth telling the user about.
    """
    available, reason = managed_runner_availability(project, create_user)
    assert available is False
    assert reason == ManagedRunnerReason.NO_RUNNER_FOR_PROJECT


@override_settings(**MANAGED_SETTINGS)
def test_enrolled_but_offline_reports_not_connected(project, create_user, openhub_lane, bundled_runner):
    bundled_runner.status = RunnerStatus.OFFLINE
    bundled_runner.save(update_fields=["status"])
    available, reason = managed_runner_availability(project, create_user)
    assert available is False
    assert reason == ManagedRunnerReason.NOT_CONNECTED
    # Still *enrolled* — that is what stops the scheduler bouncing the issue.
    assert enrolled_managed_runners(project, create_user).exists()


@override_settings(**MANAGED_SETTINGS)
def test_stale_heartbeat_counts_as_offline(project, create_user, openhub_lane, bundled_runner):
    """An ONLINE row whose heartbeat has lapsed is not a usable desktop.

    Without the freshness window a crashed app would look available forever
    and every run would queue against a machine that is never coming back.
    """
    bundled_runner.last_heartbeat_at = timezone.now() - timedelta(hours=1)
    bundled_runner.save(update_fields=["last_heartbeat_at"])
    available, reason = managed_runner_availability(project, create_user)
    assert available is False
    assert reason == ManagedRunnerReason.NOT_CONNECTED


@override_settings(**MANAGED_SETTINGS)
def test_revoked_runner_does_not_count_as_enrolled(project, create_user, openhub_lane, bundled_runner):
    bundled_runner.revoked_at = timezone.now()
    bundled_runner.save(update_fields=["revoked_at"])
    available, reason = managed_runner_availability(project, create_user)
    assert reason == ManagedRunnerReason.NO_RUNNER_FOR_PROJECT


@override_settings(**MANAGED_SETTINGS)
def test_another_users_bundled_runner_is_not_yours(project, create_user, create_user2, openhub_lane, bundled_runner):
    """A teammate's laptop on the same pod must never make the feature look
    available to someone whose own desktop is closed."""
    available, reason = managed_runner_availability(project, create_user2)
    assert available is False
    assert reason == ManagedRunnerReason.NO_RUNNER_FOR_PROJECT


@override_settings(**MANAGED_SETTINGS)
def test_executor_options_reports_three_entries(project, create_user, openhub_lane, bundled_runner):
    options = {o["kind"]: o for o in agent_executor_options(project, create_user)}
    assert set(options) == {
        AgentExecutorKind.CLOUD_AGENT,
        AgentExecutorKind.LOCAL_RUNNER,
        AgentExecutorKind.MANAGED_RUNNER,
    }
    assert options[AgentExecutorKind.MANAGED_RUNNER]["available"] is True
    assert options[AgentExecutorKind.MANAGED_RUNNER]["reason_code"] == ""


@override_settings(**MANAGED_SETTINGS)
def test_bundled_runner_does_not_advertise_local_availability(
    project, create_user, openhub_lane, bundled_runner
):
    """The picker must not claim a local runner exists on the strength of the
    bundled one — it only ever serves work pinned to it."""
    options = {o["kind"]: o for o in agent_executor_options(project, create_user)}
    assert options[AgentExecutorKind.LOCAL_RUNNER]["available"] is False
    assert options[AgentExecutorKind.LOCAL_RUNNER]["reason_code"] == "no_local_runner"

    # A genuine local runner flips it back on, proving the exclusion is by
    # provisioning and not an accident of the fixture.
    make_runner(owner=create_user, project=project, provisioning=RunnerProvisioning.MANUAL)
    options = {o["kind"]: o for o in agent_executor_options(project, create_user)}
    assert options[AgentExecutorKind.LOCAL_RUNNER]["available"] is True


@override_settings(**MANAGED_SETTINGS)
def test_bundled_runners_excluded_from_max_per_user(project, create_user, bundled_runner, manual_runner):
    """Pi Dash provisions bundled runners itself; they must not eat into the
    allowance a user has for machines they installed."""
    from pi_dash.runner.services import matcher

    assert matcher.count_active(create_user.id, project.workspace_id) == 1
    assert matcher.can_register_another(create_user.id, project.workspace_id) is True

    for _ in range(Runner.MAX_PER_USER - 1):
        make_runner(owner=create_user, project=project, provisioning=RunnerProvisioning.MANUAL)
    assert matcher.count_active(create_user.id, project.workspace_id) == Runner.MAX_PER_USER
    assert matcher.can_register_another(create_user.id, project.workspace_id) is False
