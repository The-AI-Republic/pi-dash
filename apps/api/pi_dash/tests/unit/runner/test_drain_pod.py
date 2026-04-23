# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the pod-scoped matcher helpers and ``drain_pod``.

Covers ``select_runner_in_pod``, ``next_queued_run_for_pod``, and
``drain_pod`` from ``pi_dash.runner.services.matcher``.
See ``.ai_design/issue_runner/design.md`` §6.3.
"""

from __future__ import annotations

from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import matcher


@pytest.fixture
def pod(workspace):
    return Pod.default_for_workspace(workspace)


def _make_runner(user, workspace, pod, name, online=True, heartbeat_ago_s=1):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        credential_hash=f"h-{name}",
        credential_fingerprint=name[:16].ljust(16, "x")[:16],
        status=(
            RunnerStatus.ONLINE if online else RunnerStatus.OFFLINE
        ),
        last_heartbeat_at=(
            timezone.now() - timezone.timedelta(seconds=heartbeat_ago_s)
            if online
            else None
        ),
    )


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    """Replace the WS send so tests don't need redis/channels.

    Patches at the source (``pubsub.send_to_runner``) because matcher.py
    imports it lazily inside ``drain_pod`` to avoid an import cycle.
    """
    with patch(
        "pi_dash.runner.services.pubsub.send_to_runner"
    ) as mock:
        yield mock


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    """Run ``transaction.on_commit`` callbacks inline.

    pytest-django wraps each test in a transaction that gets rolled back,
    which means ``on_commit`` callbacks never fire. Patching the hook to
    execute immediately lets us verify side effects (WS dispatch, drain
    refiring) without switching the whole test to
    ``django_db(transaction=True)``.
    """
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        yield


# ---------------- select_runner_in_pod ----------------


@pytest.mark.unit
def test_select_runner_in_pod_none_when_empty(db, pod):
    from django.db import transaction

    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None


@pytest.mark.unit
def test_select_runner_in_pod_picks_freshest_heartbeat(
    db, create_user, workspace, pod
):
    from django.db import transaction

    _make_runner(create_user, workspace, pod, "old", heartbeat_ago_s=20)
    fresh = _make_runner(create_user, workspace, pod, "fresh", heartbeat_ago_s=1)
    with transaction.atomic():
        picked = matcher.select_runner_in_pod(pod)
    assert picked.pk == fresh.pk


@pytest.mark.unit
def test_select_runner_in_pod_excludes_busy(db, create_user, workspace, pod):
    from django.db import transaction

    busy = _make_runner(create_user, workspace, pod, "busy")
    AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        runner=busy,
        status=AgentRunStatus.RUNNING,
        prompt="x",
    )
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None


@pytest.mark.unit
def test_select_runner_in_pod_excludes_other_pod(
    db, create_user, workspace
):
    """Runners in a different pod in the same workspace are not selected."""
    from django.db import transaction

    pod_a = Pod.objects.create(
        workspace=workspace, name="a", created_by=create_user
    )
    pod_b = Pod.objects.create(
        workspace=workspace, name="b", created_by=create_user
    )
    _make_runner(create_user, workspace, pod_b, "only-in-b")
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod_a) is None


@pytest.mark.unit
def test_select_runner_in_pod_skips_stale_heartbeat(
    db, create_user, workspace, pod
):
    from django.db import transaction

    _make_runner(create_user, workspace, pod, "stale", heartbeat_ago_s=120)
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None


# ---------------- next_queued_run_for_pod ----------------


@pytest.mark.unit
def test_next_queued_run_fifo(db, create_user, workspace, pod):
    from django.db import transaction

    first = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="first",
    )
    AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="second",
    )
    with transaction.atomic():
        assert matcher.next_queued_run_for_pod(pod).pk == first.pk


# ---------------- drain_pod ----------------


@pytest.mark.unit
def test_drain_pod_assigns_queued_to_idle_runner(
    db, create_user, workspace, pod, _stub_send_to_runner
):
    runner = _make_runner(create_user, workspace, pod, "r1")
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="go",
    )
    n = matcher.drain_pod(pod)
    assert n == 1
    run.refresh_from_db()
    assert run.status == AgentRunStatus.ASSIGNED
    assert run.runner_id == runner.pk
    # Billing capture: AgentRun.owner now reflects the runner's owner.
    assert run.owner_id == runner.owner_id
    # Dispatch WS call was fired.
    assert _stub_send_to_runner.called


@pytest.mark.unit
def test_drain_pod_stops_when_all_runners_busy(
    db, create_user, workspace, pod, _stub_send_to_runner
):
    _make_runner(create_user, workspace, pod, "only-runner")
    run_a = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="a",
    )
    run_b = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="b",
    )
    n = matcher.drain_pod(pod)
    assert n == 1  # Only the one runner got filled.
    run_a.refresh_from_db()
    run_b.refresh_from_db()
    statuses = sorted([run_a.status, run_b.status])
    assert statuses == [AgentRunStatus.ASSIGNED, AgentRunStatus.QUEUED]


@pytest.mark.unit
def test_drain_pod_noop_when_no_runners(
    db, create_user, workspace, pod, _stub_send_to_runner
):
    AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="orphan",
    )
    n = matcher.drain_pod(pod)
    assert n == 0
    assert not _stub_send_to_runner.called


@pytest.mark.unit
def test_drain_pod_by_id_returns_zero_when_missing(db):
    import uuid

    assert matcher.drain_pod_by_id(uuid.uuid4()) == 0
