# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Integration tests for the Pod CRUD endpoints (web app)."""

from __future__ import annotations

from uuid import uuid4

import pytest
from django.urls import reverse
from rest_framework import status

from pi_dash.db.models import User, Workspace, WorkspaceMember
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)


@pytest.fixture
def member_user(db):
    """A second user with role=Member (15) in the test workspace."""
    unique = uuid4().hex[:8]
    user = User.objects.create(
        email=f"member-{unique}@pi-dash.so",
        username=f"member_{unique}",
        first_name="M",
        last_name="ember",
    )
    user.set_password("pw")
    user.save()
    return user


@pytest.fixture
def member_in_workspace(member_user, workspace):
    WorkspaceMember.objects.create(workspace=workspace, member=member_user, role=15)
    return member_user


@pytest.fixture
def member_session_client(api_client, member_in_workspace):
    api_client.force_authenticate(user=member_in_workspace)
    return api_client


@pytest.fixture
def second_workspace_with_member(db, create_user):
    ws = Workspace.objects.create(
        name="OtherWS", owner=create_user, slug="other-ws"
    )
    WorkspaceMember.objects.create(workspace=ws, member=create_user, role=20)
    return ws


# ---------------- list / create ----------------


@pytest.mark.unit
def test_list_pods_requires_workspace_param(db, session_client):
    resp = session_client.get("/api/runners/pods/")
    assert resp.status_code == status.HTTP_400_BAD_REQUEST


@pytest.mark.unit
def test_list_pods_returns_workspace_pods(
    db, session_client, workspace
):
    resp = session_client.get(
        "/api/runners/pods/", {"workspace": str(workspace.id)}
    )
    assert resp.status_code == status.HTTP_200_OK
    data = resp.data
    assert len(data) == 1  # auto-created default
    assert data[0]["is_default"] is True
    assert data[0]["name"] == "Test Workspace-pod"


@pytest.mark.unit
def test_non_member_cannot_list_pods(
    db, session_client, second_workspace_with_member
):
    # session_client is the workspace fixture's owner; second_workspace_with_member
    # is owned by the same user, so they ARE a member. Use a fresh user instead.
    from rest_framework.test import APIClient

    other = User.objects.create(
        email=f"out-{uuid4().hex[:8]}@pi-dash.so",
        username=f"out_{uuid4().hex[:8]}",
    )
    other.set_password("pw")
    other.save()
    client = APIClient()
    client.force_authenticate(user=other)
    resp = client.get(
        "/api/runners/pods/",
        {"workspace": str(second_workspace_with_member.id)},
    )
    assert resp.status_code == status.HTTP_403_FORBIDDEN


@pytest.mark.unit
def test_admin_can_create_pod(db, session_client, workspace):
    resp = session_client.post(
        "/api/runners/pods/",
        {
            "workspace": str(workspace.id),
            "name": "gpu-pod",
            "description": "for GPU jobs",
        },
        format="json",
    )
    assert resp.status_code == status.HTTP_201_CREATED
    assert resp.data["name"] == "gpu-pod"
    assert resp.data["is_default"] is False  # only first auto-pod is default


@pytest.mark.unit
def test_member_cannot_create_pod(
    db, member_session_client, workspace
):
    resp = member_session_client.post(
        "/api/runners/pods/",
        {"workspace": str(workspace.id), "name": "x"},
        format="json",
    )
    assert resp.status_code == status.HTTP_403_FORBIDDEN


# ---------------- detail / patch ----------------


@pytest.mark.unit
def test_member_can_view_pod_detail(
    db, member_session_client, workspace
):
    pod = Pod.default_for_workspace(workspace)
    resp = member_session_client.get(f"/api/runners/pods/{pod.id}/")
    assert resp.status_code == status.HTTP_200_OK


@pytest.mark.unit
def test_admin_can_rename_pod(db, session_client, workspace):
    pod = Pod.default_for_workspace(workspace)
    resp = session_client.patch(
        f"/api/runners/pods/{pod.id}/",
        {"name": "renamed"},
        format="json",
    )
    assert resp.status_code == status.HTTP_200_OK
    assert resp.data["name"] == "renamed"


@pytest.mark.unit
def test_member_cannot_rename_pod_they_did_not_create(
    db, member_session_client, workspace
):
    pod = Pod.default_for_workspace(workspace)
    resp = member_session_client.patch(
        f"/api/runners/pods/{pod.id}/",
        {"name": "hacked"},
        format="json",
    )
    assert resp.status_code == status.HTTP_403_FORBIDDEN


@pytest.mark.unit
def test_promoting_pod_to_default_demotes_previous(
    db, session_client, workspace, create_user
):
    new_pod = Pod.objects.create(
        workspace=workspace, name="alt", created_by=create_user
    )
    old_default = Pod.default_for_workspace(workspace)
    resp = session_client.patch(
        f"/api/runners/pods/{new_pod.id}/",
        {"is_default": True},
        format="json",
    )
    assert resp.status_code == status.HTTP_200_OK
    assert resp.data["is_default"] is True
    old_default.refresh_from_db()
    assert old_default.is_default is False


# ---------------- soft-delete guards ----------------


@pytest.mark.unit
def test_delete_blocked_when_pod_has_runners(
    db, session_client, workspace, create_user
):
    pod = Pod.objects.create(
        workspace=workspace, name="with-runner", created_by=create_user
    )
    Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="r-in-pod",
        credential_hash="h",
        credential_fingerprint="f",
    )
    resp = session_client.delete(f"/api/runners/pods/{pod.id}/")
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["code"] == "pod_has_runners"


@pytest.mark.unit
def test_delete_blocked_when_pod_has_active_runs(
    db, session_client, workspace, create_user
):
    pod = Pod.objects.create(
        workspace=workspace, name="with-active", created_by=create_user
    )
    AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="x",
    )
    resp = session_client.delete(f"/api/runners/pods/{pod.id}/")
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["code"] == "pod_has_active_runs"


@pytest.mark.unit
def test_delete_blocked_for_last_pod_in_workspace(
    db, session_client, workspace
):
    pod = Pod.default_for_workspace(workspace)
    resp = session_client.delete(f"/api/runners/pods/{pod.id}/")
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["code"] == "last_pod_in_workspace"


@pytest.mark.unit
def test_delete_succeeds_when_guards_satisfied(
    db, session_client, workspace, create_user
):
    second = Pod.objects.create(
        workspace=workspace, name="second", created_by=create_user
    )
    resp = session_client.delete(f"/api/runners/pods/{second.id}/")
    assert resp.status_code == status.HTTP_204_NO_CONTENT
    second.refresh_from_db()
    assert second.deleted_at is not None
