# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import matcher


@pytest.fixture
def online_runner(db, create_user, workspace):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        name="laptop",
        credential_hash="h",
        credential_fingerprint="f",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture
def queued_run(db, create_user, workspace):
    return AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="do X",
        status=AgentRunStatus.QUEUED,
    )


@pytest.mark.unit
def test_matcher_picks_online_idle_runner(db, online_runner, queued_run):
    assert matcher.select_runner_for_run(queued_run) == online_runner


@pytest.mark.unit
def test_matcher_ignores_offline(db, online_runner, queued_run):
    online_runner.status = RunnerStatus.OFFLINE
    online_runner.save(update_fields=["status"])
    assert matcher.select_runner_for_run(queued_run) is None


@pytest.mark.unit
def test_matcher_ignores_busy_runner(db, online_runner, queued_run):
    AgentRun.objects.create(
        owner=queued_run.owner,
        workspace=queued_run.workspace,
        prompt="already in flight",
        runner=online_runner,
        status=AgentRunStatus.RUNNING,
    )
    assert matcher.select_runner_for_run(queued_run) is None


@pytest.mark.unit
def test_cap_enforced(db, create_user, workspace):
    for i in range(Runner.MAX_PER_USER):
        Runner.objects.create(
            owner=create_user,
            workspace=workspace,
            name=f"r{i}",
            credential_hash=f"h{i}",
            credential_fingerprint="f" * 12,
        )
    assert matcher.can_register_another(create_user.id, workspace.id) is False
    assert matcher.count_active(create_user.id, workspace.id) == Runner.MAX_PER_USER
