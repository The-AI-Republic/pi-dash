# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for operator pause / resume of a runner.

A paused runner stays connected (``status`` and heartbeat untouched) but every
matcher path skips it and chat to it is refused. Pausing sends queued local
runs pinned to it back to the pod queue; resume clears the pause and drains
immediately. See ``RunnerPauseEndpoint`` / ``RunnerResumeEndpoint``.
"""

from __future__ import annotations

from unittest.mock import patch

import pytest
from django.db import transaction
from django.urls import reverse
from django.utils import timezone

from pi_dash.orchestration.service import _pinned_runner_for
from pi_dash.runner.models import (
    AgentChatSession,
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import matcher
from pi_dash.runner.tasks import release_pins_on_offline_runners


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
        status=RunnerStatus.PAUSED if paused else RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
        paused_at=timezone.now() if paused else None,
    )


def _make_run(user, pod, *, pinned_runner=None, status=AgentRunStatus.QUEUED, runner=None, created_at=None):
    run = AgentRun.objects.create(
        owner=user,
        created_by=user,
        workspace=pod.workspace,
        pod=pod,
        runner=runner,
        pinned_runner=pinned_runner,
        prompt="x",
        status=status,
    )
    if created_at is not None:
        AgentRun.objects.filter(pk=run.pk).update(created_at=created_at)
    return run


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
def test_paused_runner_never_takes_its_pinned_run(db, create_user, workspace, pod):
    paused = _make_runner(create_user, workspace, pod, "paused", paused=True)
    run = _make_run(create_user, pod, pinned_runner=paused)

    assert matcher.drain_for_runner(paused) is False
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.runner_id is None


@pytest.mark.unit
def test_assignable_excludes_paused_offline_and_stale(db, create_user, workspace, pod):
    ok = _make_runner(create_user, workspace, pod, "ok")
    _make_runner(create_user, workspace, pod, "paused", paused=True)
    offline = _make_runner(create_user, workspace, pod, "offline")
    Runner.objects.filter(pk=offline.pk).update(status=RunnerStatus.OFFLINE)
    stale = _make_runner(create_user, workspace, pod, "stale")
    Runner.objects.filter(pk=stale.pk).update(last_heartbeat_at=timezone.now() - timezone.timedelta(hours=1))

    assert list(Runner.objects.assignable().filter(pod=pod)) == [ok]


# ---------------- follow-up pinning / pin release ----------------


@pytest.mark.unit
def test_follow_up_is_not_pinned_to_paused_runner(db, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod, paused=True)
    parent = _make_run(create_user, pod, runner=runner, status=AgentRunStatus.COMPLETED)
    assert _pinned_runner_for(parent) is None


@pytest.mark.unit
def test_pin_release_sweep_frees_runs_pinned_to_paused_runner(db, create_user, workspace, pod):
    paused = _make_runner(create_user, workspace, pod, "paused", paused=True)
    other = _make_runner(create_user, workspace, pod, "other")
    run = _make_run(
        create_user, pod, pinned_runner=paused, created_at=timezone.now() - timezone.timedelta(hours=1)
    )

    assert release_pins_on_offline_runners() == 1
    run.refresh_from_db()
    assert run.pinned_runner_id is None
    assert run.runner_id == other.id
    assert run.status == AgentRunStatus.ASSIGNED


# ---------------- endpoints ----------------


@pytest.mark.unit
def test_pause_sets_paused_status(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod)
    resp = session_client.post(reverse("runner-pause", kwargs={"runner_id": runner.id}))
    assert resp.status_code == 200, resp.content
    assert resp.json()["status"] == RunnerStatus.PAUSED
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.PAUSED
    assert runner.paused_at is not None
    assert runner.paused_by_id == create_user.id


@pytest.mark.unit
def test_pause_releases_pinned_runs_to_another_runner(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod, "to-pause")
    other = _make_runner(create_user, workspace, pod, "other")
    # Busy, so the pinned follow-up is waiting rather than already assigned.
    _make_run(create_user, pod, runner=runner, status=AgentRunStatus.RUNNING)
    pinned = _make_run(create_user, pod, pinned_runner=runner)

    resp = session_client.post(reverse("runner-pause", kwargs={"runner_id": runner.id}))
    assert resp.status_code == 200, resp.content
    pinned.refresh_from_db()
    assert pinned.pinned_runner_id is None
    assert pinned.runner_id == other.id
    assert pinned.status == AgentRunStatus.ASSIGNED


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
    # The operator is told what will still run.
    assert resp.json()["in_flight_runs"] == [{"id": str(in_flight.id), "status": AgentRunStatus.RUNNING}]


@pytest.mark.unit
@pytest.mark.parametrize("endpoint", ["runner-pause", "runner-resume"])
@pytest.mark.parametrize(
    "revoked_fields",
    [
        {"status": RunnerStatus.REVOKED, "revoked_at": "now"},
        # Legacy / partial revokes may have set only one marker.
        {"status": RunnerStatus.REVOKED},
        {"revoked_at": "now"},
    ],
)
def test_pause_and_resume_reject_revoked_runner(
    db, session_client, create_user, workspace, pod, endpoint, revoked_fields
):
    runner = _make_runner(create_user, workspace, pod, paused=endpoint == "runner-resume")
    fields = {k: (timezone.now() if v == "now" else v) for k, v in revoked_fields.items()}
    Runner.objects.filter(pk=runner.pk).update(**fields)
    resp = session_client.post(reverse(endpoint, kwargs={"runner_id": runner.id}))
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
    # Assignment leaves the status alone; the run start or next poll sets BUSY.
    assert runner.status == RunnerStatus.ONLINE


@pytest.mark.unit
def test_resume_idle_runner_goes_online(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod, paused=True)
    session_client.post(reverse("runner-resume", kwargs={"runner_id": runner.id}))
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.ONLINE


@pytest.mark.unit
def test_resume_runner_with_stale_heartbeat_goes_offline(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod, paused=True)
    Runner.objects.filter(pk=runner.pk).update(last_heartbeat_at=timezone.now() - timezone.timedelta(hours=1))
    session_client.post(reverse("runner-resume", kwargs={"runner_id": runner.id}))
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.OFFLINE


@pytest.mark.unit
def test_resume_runner_still_finishing_a_run_goes_busy(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod, paused=True)
    _make_run(create_user, pod, runner=runner, status=AgentRunStatus.RUNNING)
    session_client.post(reverse("runner-resume", kwargs={"runner_id": runner.id}))
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.BUSY


@pytest.mark.unit
def test_resume_unpaused_runner_does_not_drain(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod)
    with patch("pi_dash.runner.views.runners.drain_for_runner_by_id") as mock_drain:
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


# ---------------- connectivity writers ----------------


@pytest.mark.unit
def test_session_open_and_close_leave_pause_alone(db, create_user, workspace, pod):
    from pi_dash.runner.services.session_service import mark_runner_offline, mark_runner_online

    runner = _make_runner(create_user, workspace, pod, paused=True)
    Runner.objects.filter(pk=runner.pk).update(last_heartbeat_at=None)
    mark_runner_online(runner.id)
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.PAUSED
    # The heartbeat still records that the daemon is alive.
    assert runner.last_heartbeat_at is not None
    mark_runner_offline(runner.id)
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.PAUSED


@pytest.mark.unit
def test_migration_backfills_paused_status(db, create_user, workspace, pod):
    import importlib

    from django.apps import apps as django_apps

    migration = importlib.import_module("pi_dash.runner.migrations.0032_runner_paused_status")
    paused = _make_runner(create_user, workspace, pod, "was-paused")
    Runner.objects.filter(pk=paused.pk).update(paused_at=timezone.now(), status=RunnerStatus.BUSY)
    revoked = _make_runner(create_user, workspace, pod, "revoked")
    Runner.objects.filter(pk=revoked.pk).update(
        paused_at=timezone.now(), status=RunnerStatus.REVOKED, revoked_at=timezone.now()
    )
    untouched = _make_runner(create_user, workspace, pod, "untouched")

    migration.forwards(django_apps, None)

    assert Runner.objects.get(pk=paused.pk).status == RunnerStatus.PAUSED
    assert Runner.objects.get(pk=revoked.pk).status == RunnerStatus.REVOKED
    assert Runner.objects.get(pk=untouched.pk).status == RunnerStatus.ONLINE


# ---------------- chat ----------------


@pytest.mark.unit
def test_chat_session_create_rejected_on_paused_runner(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod, paused=True)
    resp = session_client.post(
        "/api/runners/chat/sessions/",
        {"workspace": str(workspace.id), "runner": str(runner.id)},
        format="json",
    )
    assert resp.status_code == 409, resp.content
    assert resp.json()["error"] == "runner_paused"


@pytest.mark.unit
def test_chat_message_rejected_on_paused_runner(db, session_client, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod)
    session = AgentChatSession.objects.create(workspace=workspace, runner=runner, created_by=create_user, pod=pod)
    Runner.objects.filter(pk=runner.pk).update(status=RunnerStatus.PAUSED, paused_at=timezone.now())
    with patch("pi_dash.runner.services.chat.send_to_runner") as send:
        resp = session_client.post(
            f"/api/runners/chat/sessions/{session.id}/messages/", {"content": "hi"}, format="json"
        )
        warm = session_client.post(f"/api/runners/chat/sessions/{session.id}/warm/")
    assert resp.status_code == 409, resp.content
    assert resp.json()["error"] == "runner_paused"
    assert warm.status_code == 409, warm.content
    send.assert_not_called()


# ---------------- metrics ----------------


@pytest.mark.unit
def test_metrics_report_paused_separately_from_online(db, client, create_user, workspace, pod):
    _make_runner(create_user, workspace, pod, "online")
    _make_runner(create_user, workspace, pod, "paused-1", paused=True)
    _make_runner(create_user, workspace, pod, "paused-2", paused=True)
    busy = _make_runner(create_user, workspace, pod, "busy")
    Runner.objects.filter(pk=busy.pk).update(status=RunnerStatus.BUSY)

    body = client.get("/api/v1/runner/metrics/").content.decode()
    assert "pi_dash_runner_online 1\n" in body
    assert "pi_dash_runner_busy 1\n" in body
    assert "pi_dash_runner_paused 2\n" in body
