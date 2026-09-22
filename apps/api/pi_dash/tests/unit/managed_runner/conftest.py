# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Fixtures for the desktop-bundled managed runner.

The managed runner differs from a hand-installed one in exactly two stored
facts — ``Runner.provisioning`` and the machine it hangs off — so these
fixtures build the *same* rows the real enrollment path writes rather than
mocking the policy layer. Tests that assert "a bundled runner is excluded
from X" are then testing the real query, not a stub.
"""

from __future__ import annotations

import pytest
from django.utils import timezone

from pi_dash.runner.models import DevMachine, Runner, RunnerProvisioning, RunnerStatus

#: Settings that make the managed runner admissible at the instance level.
#: Individual tests still control per-viewer availability through the LLM
#: profile and runner rows.
MANAGED_SETTINGS = {"MANAGED_RUNNER_ENABLED": True}


@pytest.fixture
def openhub_lane(monkeypatch):
    """Pretend this build has a model lane the desktop engine can use.

    CE ships only BYOK, which the desktop deliberately does not serve, so
    without this every availability check would stop at the provider gate and
    the runner-level assertions would never be reached. Patching the seam is
    the honest stand-in for the cloud overlay: it is the same single function
    the overlay replaces.
    """
    from pi_dash.ee.assistant import model_provider

    def _profile(user):
        return model_provider.AgentModelProfile(
            available=True,
            lane="openhub",
            base_url="https://gateway.example.com/v1",
            model="test-model",
        )

    monkeypatch.setattr(model_provider, "agent_model_profile_for_user", _profile)
    return _profile


@pytest.fixture
def byok_lane(monkeypatch):
    """A user whose provider is BYOK — supported everywhere except the desktop."""
    from pi_dash.ee.assistant import model_provider
    from pi_dash.managed_runner.errors import ManagedRunnerReason

    def _profile(user):
        return model_provider.AgentModelProfile(
            available=False,
            lane="byok",
            reason_code=ManagedRunnerReason.BYOK_UNSUPPORTED,
        )

    monkeypatch.setattr(model_provider, "agent_model_profile_for_user", _profile)
    return _profile


@pytest.fixture
def create_user2(db):
    """A second workspace member — used to prove that "someone's desktop" is
    always a specific person's, never anyone's on the pod."""
    from pi_dash.db.models import User

    user = User.objects.create(
        email="teammate@example.com",
        username="teammate",
        first_name="Team",
        last_name="Mate",
    )
    user.set_password("test-password")
    user.save()
    return user


@pytest.fixture
def desktop_machine(create_user):
    """A DevMachine the desktop app provisioned."""
    return DevMachine.objects.create(
        owner=create_user,
        host_label="rich-laptop",
        label="rich-laptop",
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        last_seen_at=timezone.now(),
    )


def make_runner(*, owner, project, provisioning, status=RunnerStatus.ONLINE, dev_machine=None, name=None):
    """Create a runner on ``project``'s default pod.

    ``last_heartbeat_at`` is set for ONLINE runners so they pass the matcher's
    freshness window; OFFLINE ones deliberately leave it stale, which is what
    "the laptop is closed" looks like in the database.
    """
    pod = project.pods.get(is_default=True)
    return Runner.objects.create(
        owner=owner,
        workspace=project.workspace,
        pod=pod,
        dev_machine=dev_machine,
        name=name or f"{provisioning}-{Runner.objects.count()}",
        host_label="rich-laptop",
        provisioning=provisioning,
        status=status,
        last_heartbeat_at=timezone.now() if status == RunnerStatus.ONLINE else None,
        enrolled_at=timezone.now(),
    )


@pytest.fixture
def bundled_runner(create_user, project, desktop_machine):
    """An online desktop-bundled runner owned by the test user."""
    return make_runner(
        owner=create_user,
        project=project,
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        dev_machine=desktop_machine,
        name="desktop-rich-laptop",
    )


@pytest.fixture
def manual_runner(create_user, project):
    """An online runner the user installed themselves."""
    return make_runner(
        owner=create_user,
        project=project,
        provisioning=RunnerProvisioning.MANUAL,
        name="workstation",
    )


@pytest.fixture
def issue_for_project(project, workspace, create_user):
    """A plain issue on the default project, for preflight/scheduler tests."""
    from pi_dash.db.models import Issue, ProjectMember, State

    state = State.objects.create(name="Todo", group="unstarted", project=project)
    ProjectMember.objects.get_or_create(
        workspace=workspace,
        project=project,
        member=create_user,
        defaults={"role": 20},
    )
    return Issue.objects.create(
        workspace=workspace,
        project=project,
        state=state,
        name="Managed task",
        created_by=create_user,
    )
