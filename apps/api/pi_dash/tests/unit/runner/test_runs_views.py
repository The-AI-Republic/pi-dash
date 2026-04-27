# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Integration tests for the run-creation endpoint after Phase 3 wiring."""

from __future__ import annotations

from unittest.mock import patch
from uuid import uuid4

import pytest
from rest_framework import status

from pi_dash.db.models import User, Workspace, WorkspaceMember
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
)


@pytest.fixture
def second_workspace(db, create_user):
    ws = Workspace.objects.create(
        name="OtherWS", owner=create_user, slug="other-ws-runs"
    )
    WorkspaceMember.objects.create(workspace=ws, member=create_user, role=20)
    return ws


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        yield


@pytest.fixture(autouse=True)
def _on_commit_immediate():
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        yield


@pytest.mark.unit
def test_post_run_validates_workspace_membership(
    db, api_client, second_workspace
):
    outsider = User.objects.create(
        email=f"out-{uuid4().hex[:8]}@example.com",
        username=f"out_{uuid4().hex[:8]}",
    )
    outsider.set_password("pw")
    outsider.save()
    api_client.force_authenticate(user=outsider)
    resp = api_client.post(
        "/api/runners/runs/",
        {"prompt": "x", "workspace": str(second_workspace.id)},
        format="json",
    )
    assert resp.status_code == status.HTTP_403_FORBIDDEN
    assert resp.data["code"] == "not_workspace_member"


@pytest.mark.unit
def test_post_run_creates_with_workspace_default_pod(
    db, session_client, workspace
):
    resp = session_client.post(
        "/api/runners/runs/",
        {"prompt": "do work", "workspace": str(workspace.id)},
        format="json",
    )
    assert resp.status_code == status.HTTP_201_CREATED
    run_id = resp.data["id"]
    run = AgentRun.objects.get(id=run_id)
    assert run.pod_id == Pod.default_for_workspace(workspace).id
    assert run.created_by_id == workspace.owner_id


@pytest.mark.unit
def test_post_run_rejects_pod_in_other_workspace(
    db, session_client, workspace, second_workspace
):
    other_pod = Pod.default_for_workspace(second_workspace)
    resp = session_client.post(
        "/api/runners/runs/",
        {
            "prompt": "x",
            "workspace": str(workspace.id),
            "pod": str(other_pod.id),
        },
        format="json",
    )
    assert resp.status_code == status.HTTP_400_BAD_REQUEST
    assert resp.data["code"] == "pod_workspace_mismatch"


@pytest.mark.unit
def test_post_run_ignores_request_body_created_by(
    db, session_client, workspace
):
    """Caller can't impersonate someone else by passing created_by in the body."""
    spoofed = User.objects.create(
        email=f"spoof-{uuid4().hex[:8]}@example.com",
        username=f"spoof_{uuid4().hex[:8]}",
    )
    spoofed.set_password("pw")
    spoofed.save()
    resp = session_client.post(
        "/api/runners/runs/",
        {
            "prompt": "x",
            "workspace": str(workspace.id),
            "created_by": spoofed.id,
        },
        format="json",
    )
    assert resp.status_code == status.HTTP_201_CREATED
    run = AgentRun.objects.get(id=resp.data["id"])
    # created_by reflects the authenticated user, not the body field.
    assert run.created_by_id == workspace.owner_id


@pytest.mark.unit
def test_get_runs_lists_by_created_by(db, session_client, workspace):
    AgentRun.objects.create(
        workspace=workspace,
        created_by=workspace.owner,
        pod=Pod.default_for_workspace(workspace),
        prompt="mine",
    )
    other = User.objects.create(
        email=f"o-{uuid4().hex[:8]}@example.com",
        username=f"o_{uuid4().hex[:8]}",
    )
    other.set_password("pw")
    other.save()
    AgentRun.objects.create(
        workspace=workspace,
        created_by=other,
        pod=Pod.default_for_workspace(workspace),
        prompt="not mine",
    )
    resp = session_client.get("/api/runners/runs/")
    assert resp.status_code == status.HTTP_200_OK
    prompts = [r["prompt"] for r in resp.data]
    assert "mine" in prompts
    assert "not mine" not in prompts


# ---------------------------------------------------------------------------
# AgentRunReleasePinEndpoint
#
# Operator escape hatch for a stuck pin: the pinned runner is offline
# indefinitely, and the human chooses to give up native session resume so
# any other runner can pick the run up. See §5.7 of design doc.
# ---------------------------------------------------------------------------


def _make_pinned_run(workspace, *, parent_thread_id=None):
    from django.utils import timezone

    from pi_dash.runner.models import Runner, RunnerStatus

    pod = Pod.default_for_workspace(workspace)
    runner = Runner.objects.create(
        owner=workspace.owner,
        workspace=workspace,
        pod=pod,
        name="pinR",
        credential_hash="h",
        credential_fingerprint="f" * 12,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )
    parent = None
    if parent_thread_id is not None:
        parent = AgentRun.objects.create(
            workspace=workspace,
            created_by=workspace.owner,
            pod=pod,
            runner=runner,
            thread_id=parent_thread_id,
            status=AgentRunStatus.PAUSED_AWAITING_INPUT,
            prompt="prior",
            started_at=timezone.now() - timezone.timedelta(minutes=5),
        )
    run = AgentRun.objects.create(
        workspace=workspace,
        created_by=workspace.owner,
        pod=pod,
        parent_run=parent,
        pinned_runner=runner,
        status=AgentRunStatus.QUEUED,
        prompt="continuation",
    )
    return run, parent, runner


@pytest.mark.unit
def test_release_pin_clears_pin_and_parent_thread_id(
    db, session_client, workspace
):
    run, parent, runner = _make_pinned_run(
        workspace, parent_thread_id="sess_alive"
    )
    resp = session_client.post(
        f"/api/runners/runs/{run.id}/release-pin/",
        {},
        format="json",
    )
    assert resp.status_code == status.HTTP_200_OK
    run.refresh_from_db()
    parent.refresh_from_db()
    assert run.pinned_runner_id is None
    # Status may flip to ASSIGNED if a runner is online and idle (drain
    # fires on commit). The endpoint contract is "drop the pin without
    # cancelling," not "stay QUEUED." Asserting the pin is what matters.
    assert run.status in (AgentRunStatus.QUEUED, AgentRunStatus.ASSIGNED)
    # Parent's session id is wiped so the next runner doesn't get a stale
    # resume hint.
    assert parent.thread_id == ""


@pytest.mark.unit
def test_release_pin_returns_409_when_not_queued(
    db, session_client, workspace
):
    run, _, _ = _make_pinned_run(workspace)
    AgentRun.objects.filter(pk=run.pk).update(status=AgentRunStatus.RUNNING)
    resp = session_client.post(
        f"/api/runners/runs/{run.id}/release-pin/", {}, format="json"
    )
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["error"] == "run not queued"


@pytest.mark.unit
def test_release_pin_returns_409_when_not_pinned(
    db, session_client, workspace
):
    run, _, _ = _make_pinned_run(workspace)
    AgentRun.objects.filter(pk=run.pk).update(pinned_runner=None)
    resp = session_client.post(
        f"/api/runners/runs/{run.id}/release-pin/", {}, format="json"
    )
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["error"] == "run not pinned"


@pytest.mark.unit
def test_release_pin_404_for_run_in_other_workspace(
    db, api_client, workspace, second_workspace
):
    """A user who isn't authorized for the run must not see it exist."""
    run, _, _ = _make_pinned_run(workspace)
    outsider = User.objects.create(
        email=f"o-{uuid4().hex[:8]}@example.com",
        username=f"o_{uuid4().hex[:8]}",
    )
    outsider.set_password("pw")
    outsider.save()
    api_client.force_authenticate(user=outsider)
    resp = api_client.post(
        f"/api/runners/runs/{run.id}/release-pin/", {}, format="json"
    )
    assert resp.status_code == status.HTTP_404_NOT_FOUND
