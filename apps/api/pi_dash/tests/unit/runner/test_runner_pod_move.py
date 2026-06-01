# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for moving a runner between pods via ``PATCH /api/runners/<id>/``.

The move is blocked while the runner has a non-terminal run *bound* to it —
one it is serving (``runner=``) or one reserved for it (``pinned_runner=``,
e.g. a queued/paused follow-up). A run's pod FK is immutable, so re-pointing
the runner would strand that run in the old pod. Unpinned queued runs are
not bound to the runner, so they don't block. See
``RunnerDetailEndpoint.patch``.
"""

from __future__ import annotations

import pytest
from django.urls import reverse
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def other_pod(project, create_user):
    return Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name=f"{project.identifier}_other",
        created_by=create_user,
    )


def _make_runner(user, workspace, pod, name="r1"):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _make_run(user, pod, *, runner=None, pinned_runner=None, status=AgentRunStatus.RUNNING):
    return AgentRun.objects.create(
        owner=user,
        created_by=user,
        workspace=pod.workspace,
        pod=pod,
        runner=runner,
        pinned_runner=pinned_runner,
        prompt="x",
        status=status,
    )


@pytest.mark.unit
def test_move_runner_to_another_pod_succeeds_when_idle(
    db, session_client, create_user, workspace, pod, other_pod
):
    runner = _make_runner(create_user, workspace, pod, "idle")
    url = reverse("runner-detail", kwargs={"runner_id": runner.id})
    resp = session_client.patch(url, {"pod": str(other_pod.id)}, format="json")
    assert resp.status_code == 200, resp.content
    runner.refresh_from_db()
    assert runner.pod_id == other_pod.id


@pytest.mark.unit
def test_move_runner_blocked_while_serving_active_run(
    db, session_client, create_user, workspace, pod, other_pod
):
    runner = _make_runner(create_user, workspace, pod, "busy")
    _make_run(create_user, runner.pod, runner=runner, status=AgentRunStatus.RUNNING)
    url = reverse("runner-detail", kwargs={"runner_id": runner.id})
    resp = session_client.patch(url, {"pod": str(other_pod.id)}, format="json")
    assert resp.status_code == 409, resp.content
    assert resp.json().get("code") == "runner_busy"
    runner.refresh_from_db()
    # Pod is unchanged.
    assert runner.pod_id == pod.id


@pytest.mark.unit
def test_move_runner_blocked_with_queued_pinned_run(
    db, session_client, create_user, workspace, pod, other_pod
):
    """A queued run reserved for this runner (pinned, runner FK still null)
    is pending work bound to it — moving the runner would strand it."""
    runner = _make_runner(create_user, workspace, pod, "pinned")
    _make_run(
        create_user, runner.pod, pinned_runner=runner, status=AgentRunStatus.QUEUED
    )
    url = reverse("runner-detail", kwargs={"runner_id": runner.id})
    resp = session_client.patch(url, {"pod": str(other_pod.id)}, format="json")
    assert resp.status_code == 409, resp.content
    assert resp.json().get("code") == "runner_busy"
    runner.refresh_from_db()
    assert runner.pod_id == pod.id


@pytest.mark.unit
def test_unpinned_queued_run_does_not_block_move(
    db, session_client, create_user, workspace, pod, other_pod
):
    """A queued run in the pod that is NOT bound to this runner (no runner,
    no pinned_runner) can be served by any runner, so it doesn't block."""
    runner = _make_runner(create_user, workspace, pod, "unpinned")
    _make_run(create_user, runner.pod, status=AgentRunStatus.QUEUED)
    url = reverse("runner-detail", kwargs={"runner_id": runner.id})
    resp = session_client.patch(url, {"pod": str(other_pod.id)}, format="json")
    assert resp.status_code == 200, resp.content
    runner.refresh_from_db()
    assert runner.pod_id == other_pod.id


@pytest.mark.unit
def test_resend_same_pod_allowed_while_serving_active_run(
    db, session_client, create_user, workspace, pod
):
    """Re-sending the runner's current pod is a no-op, allowed even mid-run."""
    runner = _make_runner(create_user, workspace, pod, "busy-noop")
    _make_run(create_user, runner.pod, runner=runner, status=AgentRunStatus.RUNNING)
    url = reverse("runner-detail", kwargs={"runner_id": runner.id})
    resp = session_client.patch(url, {"pod": str(pod.id)}, format="json")
    assert resp.status_code == 200, resp.content
    runner.refresh_from_db()
    assert runner.pod_id == pod.id


@pytest.mark.unit
def test_paused_run_bound_to_runner_blocks_move(
    db, session_client, create_user, workspace, pod, other_pod
):
    """A paused run is non-terminal and still bound to its runner; it resumes
    on that runner, so moving the runner to another pod would strand it."""
    runner = _make_runner(create_user, workspace, pod, "paused")
    _make_run(
        create_user, runner.pod, runner=runner,
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
    )
    url = reverse("runner-detail", kwargs={"runner_id": runner.id})
    resp = session_client.patch(url, {"pod": str(other_pod.id)}, format="json")
    assert resp.status_code == 409, resp.content
    assert resp.json().get("code") == "runner_busy"
    runner.refresh_from_db()
    assert runner.pod_id == pod.id


@pytest.mark.unit
def test_rename_allowed_while_serving_active_run(
    db, session_client, create_user, workspace, pod
):
    """The busy guard is scoped to pod moves; a rename still goes through."""
    runner = _make_runner(create_user, workspace, pod, "busy-rename")
    _make_run(create_user, runner.pod, runner=runner, status=AgentRunStatus.RUNNING)
    url = reverse("runner-detail", kwargs={"runner_id": runner.id})
    resp = session_client.patch(url, {"name": "renamed"}, format="json")
    assert resp.status_code == 200, resp.content
    runner.refresh_from_db()
    assert runner.name == "renamed"
