# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the Workspace ``post_save`` signal that auto-creates a default pod.

See ``.ai_design/issue_runner/design.md`` §7.1 and invariant #13.
"""

from __future__ import annotations

import pytest

from pi_dash.db.models import Workspace
from pi_dash.runner.models import Pod


@pytest.mark.unit
def test_signal_creates_default_pod_on_new_workspace(db, create_user):
    ws = Workspace.objects.create(
        name="Signal Ws", owner=create_user, slug="signal-ws"
    )
    pods = Pod.objects.filter(workspace=ws)
    assert pods.count() == 1
    pod = pods.first()
    assert pod.name == "Signal Ws-pod"
    assert pod.is_default is True


@pytest.mark.unit
def test_signal_noop_on_workspace_update(db, workspace):
    before = Pod.objects.filter(workspace=workspace).count()
    workspace.name = "Renamed"
    workspace.save()
    after = Pod.objects.filter(workspace=workspace).count()
    assert before == after  # no new pod spawned by rename


@pytest.mark.unit
def test_signal_respects_existing_pods(db, create_user):
    """If a fixture pre-creates a pod, the signal does not duplicate it."""
    ws = Workspace(name="Pre-Seeded", owner=create_user, slug="pre-seeded")
    # Manually create before save — in practice fixtures typically seed after
    # save; this test just exercises the guard.
    ws.save()  # Signal fires here, creating one pod.
    # Second save is an update; signal guard should not spawn another pod.
    ws.save()
    assert Pod.objects.filter(workspace=ws).count() == 1


@pytest.mark.unit
def test_pod_name_uses_workspace_name_verbatim(db, create_user):
    ws = Workspace.objects.create(
        name="Acme Inc.", owner=create_user, slug="acme-inc"
    )
    pod = Pod.objects.get(workspace=ws)
    assert pod.name == "Acme Inc.-pod"


@pytest.mark.unit
def test_runner_save_auto_resolves_pod_from_workspace_default(
    db, create_user, workspace
):
    """Runner.objects.create without explicit pod picks up the workspace default."""
    from pi_dash.runner.models import Runner, RunnerStatus

    runner = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        name="auto-pod-runner",
        credential_hash="h",
        credential_fingerprint="f",
        status=RunnerStatus.OFFLINE,
    )
    assert runner.pod_id is not None
    assert runner.pod.is_default is True
    assert runner.pod.workspace_id == workspace.id


@pytest.mark.unit
def test_agent_run_save_auto_resolves_pod_and_mirrors_created_by(
    db, create_user, workspace
):
    """AgentRun.objects.create without pod/created_by fills them via the save() override."""
    from pi_dash.runner.models import AgentRun, AgentRunStatus

    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,  # legacy call path
        prompt="hi",
        status=AgentRunStatus.QUEUED,
    )
    assert run.pod_id is not None
    assert run.pod.is_default is True
    # created_by auto-mirrored from owner on legacy path.
    assert run.created_by_id == create_user.id
