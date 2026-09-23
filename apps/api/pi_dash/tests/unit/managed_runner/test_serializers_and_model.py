# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""API surface and stored shape for the managed runner.

Two things must hold no matter what a client sends: ``provisioning`` is
derived server-side (a runner that could claim to be Pi Dash-managed would
become eligible for pinned desktop work), and pinning an issue to the desktop
is refused with a reason the UI can render rather than a bare 400.
"""

from __future__ import annotations

import pytest
from django.test import override_settings

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.runner.models import AgentRun, AgentRunStatus, DevMachine, Runner, RunnerProvisioning
from pi_dash.runner.serializers import RunnerSerializer

from .conftest import MANAGED_SETTINGS

pytestmark = pytest.mark.unit


# --------------------------------------------------------------------------
# Model / migration shape
# --------------------------------------------------------------------------


def test_provisioning_defaults_to_manual(create_user, project):
    """Every pre-existing row is a manually enrolled runner; the migration's
    default must preserve that or existing installs would silently become
    "managed" and stop receiving unpinned work."""
    runner = Runner.objects.create(
        owner=create_user,
        workspace=project.workspace,
        pod=project.pods.get(is_default=True),
        name="legacy",
    )
    assert runner.provisioning == RunnerProvisioning.MANUAL

    machine = DevMachine.objects.create(owner=create_user, host_label="box")
    assert machine.provisioning == RunnerProvisioning.MANUAL


def test_executor_kind_choices_include_managed():
    assert AgentExecutorKind.MANAGED_RUNNER in AgentExecutorKind.values
    assert AgentExecutorKind.MANAGED_RUNNER.label == "Pi Dash Agent"
    # The column is 24 chars; a longer value would truncate on insert.
    assert len(AgentExecutorKind.MANAGED_RUNNER.value) <= 24


def test_check_constraint_still_rejects_a_cloud_run_with_local_assignment(
    create_user, project, bundled_runner
):
    """Widening the constraint for managed runs must not have loosened the
    cloud branch, which asserts a cloud run owns no machine assignment."""
    from django.db import IntegrityError, transaction

    with pytest.raises(IntegrityError):
        with transaction.atomic():
            AgentRun.objects.create(
                workspace=project.workspace,
                created_by=create_user,
                pod=project.pods.get(is_default=True),
                executor_kind=AgentExecutorKind.CLOUD_AGENT,
                pinned_runner=bundled_runner,
                status=AgentRunStatus.QUEUED,
                prompt="",
            )


# --------------------------------------------------------------------------
# Serializers
# --------------------------------------------------------------------------


def test_runner_serializer_exposes_provisioning_read_only(bundled_runner):
    data = RunnerSerializer(bundled_runner).data
    assert data["provisioning"] == RunnerProvisioning.DESKTOP_BUNDLED
    assert "provisioning" in RunnerSerializer.Meta.read_only_fields


def test_runner_serializer_ignores_a_client_supplied_provisioning(manual_runner):
    """A client must not be able to promote its own runner to managed."""
    serializer = RunnerSerializer(
        manual_runner,
        data={"provisioning": RunnerProvisioning.DESKTOP_BUNDLED},
        partial=True,
    )
    assert serializer.is_valid(), serializer.errors
    assert "provisioning" not in serializer.validated_data


@override_settings(**MANAGED_SETTINGS)
def test_issue_serializer_accepts_a_managed_pin_when_available(
    project, create_user, openhub_lane, bundled_runner, issue_for_project, rf
):
    from pi_dash.app.serializers.issue import IssueCreateSerializer as IssueSerializer

    request = rf.post("/")
    request.user = create_user
    serializer = IssueSerializer(
        issue_for_project,
        data={"agent_executor": AgentExecutorKind.MANAGED_RUNNER},
        partial=True,
        context={"request": request, "project_id": str(project.id)},
    )
    assert serializer.is_valid(), serializer.errors


@override_settings(**MANAGED_SETTINGS)
def test_issue_serializer_refuses_a_managed_pin_with_an_actionable_reason(
    project, create_user, byok_lane, bundled_runner, issue_for_project, rf
):
    """The error text is what the user reads; it must name the fix, not the
    internal reason code."""
    from pi_dash.app.serializers.issue import IssueCreateSerializer as IssueSerializer

    request = rf.post("/")
    request.user = create_user
    serializer = IssueSerializer(
        issue_for_project,
        data={"agent_executor": AgentExecutorKind.MANAGED_RUNNER},
        partial=True,
        context={"request": request, "project_id": str(project.id)},
    )
    assert not serializer.is_valid()
    detail = str(serializer.errors["agent_executor"])
    assert "OpenHub" in detail


@override_settings(**MANAGED_SETTINGS)
def test_issue_serializer_allows_a_pin_before_the_desktop_enrolls_the_project(
    project, create_user, openhub_lane, issue_for_project, rf
):
    """"No runner for this project yet" is a race the desktop resolves by
    enrolling and retrying — blocking on it would make the first Run on every
    new project fail."""
    from pi_dash.app.serializers.issue import IssueCreateSerializer as IssueSerializer

    request = rf.post("/")
    request.user = create_user
    serializer = IssueSerializer(
        issue_for_project,
        data={"agent_executor": AgentExecutorKind.MANAGED_RUNNER},
        partial=True,
        context={"request": request, "project_id": str(project.id)},
    )
    assert serializer.is_valid(), serializer.errors


@override_settings(MANAGED_RUNNER_ENABLED=False)
def test_project_serializer_refuses_a_managed_default_when_disabled(project, create_user, rf):
    from pi_dash.api.serializers.project import ProjectSerializer

    serializer = ProjectSerializer(
        project, data={"default_agent_executor": AgentExecutorKind.MANAGED_RUNNER}, partial=True
    )
    assert not serializer.is_valid()
    assert "default_agent_executor" in serializer.errors


@override_settings(**MANAGED_SETTINGS)
def test_project_serializer_accepts_a_managed_default_when_enabled(project):
    """A project owner may set the default before anyone installs the app —
    availability is a per-viewer question, not a project one."""
    from pi_dash.api.serializers.project import ProjectSerializer

    serializer = ProjectSerializer(
        project, data={"default_agent_executor": AgentExecutorKind.MANAGED_RUNNER}, partial=True
    )
    assert serializer.is_valid(), serializer.errors


# --------------------------------------------------------------------------
# Listings and reporting
# --------------------------------------------------------------------------


def test_engine_version_is_whitelisted_into_dev_metadata(bundled_runner):
    """Support's "which build is this user on" question: the bundled binary
    ships inside the app, so the user cannot report its version themselves."""
    from pi_dash.runner.services.session_service import apply_hello

    apply_hello(bundled_runner, {"os": "linux", "arch": "x86_64", "version": "0.1.21", "engine_version": "codex 1.2.3"})
    bundled_runner.refresh_from_db()
    assert bundled_runner.dev_metadata["codex_version"] == "codex 1.2.3"
    assert bundled_runner.runner_version == "0.1.21"


def test_unknown_session_open_keys_are_still_ignored(bundled_runner):
    """The whitelist must stay a whitelist — a runner cannot write arbitrary
    metadata onto its own row."""
    from pi_dash.runner.services.session_service import apply_hello

    apply_hello(bundled_runner, {"engine_version": "codex 1.2.3", "evil": "x" * 10})
    bundled_runner.refresh_from_db()
    assert "evil" not in bundled_runner.dev_metadata


def test_engine_version_is_length_capped(bundled_runner):
    from pi_dash.runner.services.session_service import apply_hello

    apply_hello(bundled_runner, {"engine_version": "v" * 500})
    bundled_runner.refresh_from_db()
    assert len(bundled_runner.dev_metadata["codex_version"]) <= 64


def test_apply_hello_persists_agent_kind_capability(bundled_runner):
    """The daemon reports the ``AgentKind`` it drives; we persist it as an
    ``agent:<kind>`` capability so diagnostics can identify the agent exactly."""
    from pi_dash.runner.services.session_service import apply_hello

    apply_hello(bundled_runner, {"os": "linux", "agent_kind": "muse_code"})
    bundled_runner.refresh_from_db()
    assert bundled_runner.capabilities == ["agent:muse_code"]


def test_apply_hello_without_agent_kind_leaves_capabilities_untouched(bundled_runner):
    """An older daemon omits ``agent_kind`` — a missing field must not clobber a
    previously-reported capability with an empty list."""
    from pi_dash.runner.services.session_service import apply_hello

    bundled_runner.capabilities = ["agent:codex"]
    bundled_runner.save(update_fields=["capabilities"])

    apply_hello(bundled_runner, {"os": "linux", "version": "0.1.21"})
    bundled_runner.refresh_from_db()
    assert bundled_runner.capabilities == ["agent:codex"]


def test_apply_hello_rejects_malformed_agent_kind(bundled_runner):
    """A malformed ``agent_kind`` cannot inject arbitrary text into the field."""
    from pi_dash.runner.services.session_service import apply_hello

    apply_hello(bundled_runner, {"agent_kind": "not a kind; drop table"})
    bundled_runner.refresh_from_db()
    assert bundled_runner.capabilities == []
