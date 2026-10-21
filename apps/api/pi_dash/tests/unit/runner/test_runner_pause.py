# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for operator pause / resume of a runner.

A paused runner stays connected (``status`` and heartbeat untouched) but every
matcher path skips it, so queued work — including runs pinned to it — waits
until it is resumed. Resume clears the pause and drains immediately. See
``RunnerPauseEndpoint`` / ``RunnerResumeEndpoint``.
"""

from __future__ import annotations

from unittest.mock import patch

import pytest
from django.db import transaction
from django.urls import reverse
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
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with patch("pi_dash.runner.services.pubsub.send_to_runner") as mock:
        yield mock


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


def _make_runner(user, workspace, pod, name="r1", *, paused=False):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
        paused_at=timezone.now() if paused else None,
    )


def _make_run(user, pod, *, pinned_runner=None, status=AgentRunStatus.QUEUED, runner=None):
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


# ---------------- matcher ----------------


@pytest.mark.unit
def test_select_runner_in_pod_skips_paused(db, create_user, workspace, pod):
    _make_runner(create_user, workspace, pod, "paused", paused=True)
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None


@pytest.mark.unit
def test_drain_pod_routes_around_paused_runner(db, create_user, workspace, pod):
    _make_runner(create_user, workspace, pod, "paused", paused=True)
    active = _make_runner(create_user, workspace, pod, "active")
    run = _make_run(create_user, pod)

    assert matcher.drain_pod(pod) == 1
    run.refresh_from_db()
    assert run.runner_id == active.id


@pytest.mark.unit
def test_pinned_run_waits_for_paused_runner(db, create_user, workspace, pod):
    paused = _make_runner(create_user, workspace, pod, "paused", paused=True)
    _make_runner(create_user, workspace, pod, "other")
    run = _make_run(create_user, pod, pinned_runner=paused)

    assert matcher.drain_for_runner(paused) is False
    assert matcher.drain_pod(pod) == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.runner_id is None


# ---------------- endpoints ----------------


@pytest.mark.unit
def test_pause_marks_runner_paused_without_touching_status(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod)
    resp = session_client.post(reverse("runner-pause", kwargs={"runner_id": runner.id}))
    assert resp.status_code == 200, resp.content
    assert resp.json()["paused_at"] is not None
    runner.refresh_from_db()
    assert runner.paused_at is not None
    assert runner.paused_by_id == create_user.id
    assert runner.status == RunnerStatus.ONLINE


@pytest.mark.unit
def test_pause_is_idempotent(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod, paused=True)
    original = runner.paused_at
    resp = session_client.post(reverse("runner-pause", kwargs={"runner_id": runner.id}))
    assert resp.status_code == 200, resp.content
    runner.refresh_from_db()
    assert runner.paused_at == original


@pytest.mark.unit
def test_pause_while_busy_leaves_in_flight_run_alone(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod)
    in_flight = _make_run(create_user, pod, runner=runner, status=AgentRunStatus.RUNNING)
    resp = session_client.post(reverse("runner-pause", kwargs={"runner_id": runner.id}))
    assert resp.status_code == 200, resp.content
    in_flight.refresh_from_db()
    assert in_flight.status == AgentRunStatus.RUNNING
    assert in_flight.runner_id == runner.id


@pytest.mark.unit
def test_pause_revoked_runner_conflicts(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod)
    Runner.objects.filter(pk=runner.pk).update(status=RunnerStatus.REVOKED, revoked_at=timezone.now())
    resp = session_client.post(reverse("runner-pause", kwargs={"runner_id": runner.id}))
    assert resp.status_code == 409, resp.content
    assert resp.json()["code"] == "runner_revoked"


@pytest.mark.unit
def test_resume_clears_pause_and_drains_queued_work(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod, paused=True)
    run = _make_run(create_user, pod, pinned_runner=runner)

    resp = session_client.post(reverse("runner-resume", kwargs={"runner_id": runner.id}))
    assert resp.status_code == 200, resp.content
    assert resp.json()["paused_at"] is None
    runner.refresh_from_db()
    assert runner.paused_at is None
    assert runner.paused_by_id is None
    run.refresh_from_db()
    assert run.status == AgentRunStatus.ASSIGNED
    assert run.runner_id == runner.id


@pytest.mark.unit
def test_resume_unpaused_runner_does_not_drain(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod)
    with patch("pi_dash.runner.services.matcher.drain_for_runner_by_id") as mock_drain:
        resp = session_client.post(reverse("runner-resume", kwargs={"runner_id": runner.id}))
    assert resp.status_code == 200, resp.content
    mock_drain.assert_not_called()


@pytest.mark.unit
def test_pause_requires_manage_permission(db, session_client, create_user, workspace, pod):
    from pi_dash.db.models import User

    other = User.objects.create(email="other-owner@example.com", username="other-owner")
    runner = _make_runner(other, workspace, pod, "theirs")
    resp = session_client.post(reverse("runner-pause", kwargs={"runner_id": runner.id}))
    # Private runners are invisible to non-owners, so this reads as 404.
    assert resp.status_code == 404, resp.content
    runner.refresh_from_db()
    assert runner.paused_at is None
