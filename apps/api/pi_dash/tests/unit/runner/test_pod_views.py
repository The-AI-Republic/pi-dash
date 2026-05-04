# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Integration tests for the project-scoped Pod CRUD endpoints (web app).

See ``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§6.2 for the surface this exercises.
"""

from __future__ import annotations

from uuid import uuid4

import pytest
from django.urls import reverse
from rest_framework import status

from pi_dash.db.models import User, Workspace, WorkspaceMember
from pi_dash.db.models.project import Project
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
        email=f"member-{unique}@example.com",
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
def out_of_workspace_user(db):
    unique = uuid4().hex[:8]
    user = User.objects.create(
        email=f"out-{unique}@example.com",
        username=f"out_{unique}",
    )
    user.set_password("pw")
    user.save()
    return user


# ---------------- list ----------------


@pytest.mark.unit
def test_list_pods_by_project(db, session_client, project):
    resp = session_client.get(
        "/api/runners/pods/", {"project": str(project.id)}
    )
    assert resp.status_code == status.HTTP_200_OK
    data = resp.data
    assert len(data) == 1  # auto-created default
    assert data[0]["is_default"] is True
    assert data[0]["name"] == f"{project.identifier}_pod_1"


@pytest.mark.unit
def test_list_pods_by_workspace_aggregates_across_projects(
    db, session_client, workspace, project, create_user
):
    second_proj = Project.objects.create(
        name="Other",
        identifier="OTHER",
        workspace=workspace,
        created_by=create_user,
    )
    resp = session_client.get(
        "/api/runners/pods/", {"workspace": str(workspace.id)}
    )
    assert resp.status_code == status.HTTP_200_OK
    names = {p["name"] for p in resp.data}
    assert f"{project.identifier}_pod_1" in names
    assert f"{second_proj.identifier}_pod_1" in names


@pytest.mark.unit
def test_non_member_cannot_list_pods(
    db, api_client, project, out_of_workspace_user
):
    api_client.force_authenticate(user=out_of_workspace_user)
    resp = api_client.get("/api/runners/pods/", {"project": str(project.id)})
    assert resp.status_code == status.HTTP_403_FORBIDDEN


# ---------------- create ----------------


@pytest.mark.unit
def test_admin_can_create_pod_with_full_name(db, session_client, project):
    resp = session_client.post(
        "/api/runners/pods/",
        {
            "project": str(project.id),
            "name": f"{project.identifier}_beefy",
            "description": "for GPU jobs",
        },
        format="json",
    )
    assert resp.status_code == status.HTTP_201_CREATED, resp.data
    assert resp.data["name"] == f"{project.identifier}_beefy"
    assert resp.data["is_default"] is False  # default reserved for the auto pod


@pytest.mark.unit
def test_create_pod_accepts_bare_suffix_and_auto_prefixes(
    db, session_client, project
):
    """`name=beefy` should be auto-prefixed to `{identifier}_beefy`."""
    resp = session_client.post(
        "/api/runners/pods/",
        {"project": str(project.id), "name": "beefy"},
        format="json",
    )
    assert resp.status_code == status.HTTP_201_CREATED, resp.data
    assert resp.data["name"] == f"{project.identifier}_beefy"


@pytest.mark.unit
def test_create_pod_rejects_reserved_pod_n_suffix(db, session_client, project):
    """`pod_<digits>` is reserved for system-auto-generation."""
    resp = session_client.post(
        "/api/runners/pods/",
        {"project": str(project.id), "name": "pod_2"},
        format="json",
    )
    assert resp.status_code == status.HTTP_400_BAD_REQUEST
    assert "reserved" in resp.data["error"]


@pytest.mark.unit
def test_create_pod_requires_project(db, session_client):
    resp = session_client.post(
        "/api/runners/pods/", {"name": "anything"}, format="json"
    )
    assert resp.status_code == status.HTTP_400_BAD_REQUEST


@pytest.mark.unit
def test_create_pod_unknown_project_returns_404(db, session_client):
    resp = session_client.post(
        "/api/runners/pods/",
        {"project": str(uuid4()), "name": "x"},
        format="json",
    )
    assert resp.status_code == status.HTTP_404_NOT_FOUND


@pytest.mark.unit
def test_member_cannot_create_pod(db, member_session_client, project):
    resp = member_session_client.post(
        "/api/runners/pods/",
        {"project": str(project.id), "name": "beefy"},
        format="json",
    )
    assert resp.status_code == status.HTTP_403_FORBIDDEN


# ---------------- detail / patch ----------------


@pytest.mark.unit
def test_member_can_view_pod_detail(db, member_session_client, project):
    pod = Pod.default_for_project(project)
    resp = member_session_client.get(f"/api/runners/pods/{pod.id}/")
    assert resp.status_code == status.HTTP_200_OK


@pytest.mark.unit
def test_admin_can_rename_pod(db, session_client, project, create_user):
    pod = Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name=f"{project.identifier}_old",
        created_by=create_user,
    )
    resp = session_client.patch(
        f"/api/runners/pods/{pod.id}/",
        {"name": "renamed"},  # bare suffix → auto-prefix
        format="json",
    )
    assert resp.status_code == status.HTTP_200_OK, resp.data
    assert resp.data["name"] == f"{project.identifier}_renamed"


@pytest.mark.unit
def test_rename_rejects_reserved_suffix(
    db, session_client, project, create_user
):
    pod = Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name=f"{project.identifier}_old",
        created_by=create_user,
    )
    resp = session_client.patch(
        f"/api/runners/pods/{pod.id}/",
        {"name": "pod_5"},
        format="json",
    )
    assert resp.status_code == status.HTTP_400_BAD_REQUEST


@pytest.mark.unit
def test_promoting_pod_to_default_demotes_previous_within_project(
    db, session_client, project, create_user
):
    new_pod = Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name=f"{project.identifier}_alt",
        created_by=create_user,
    )
    old_default = Pod.default_for_project(project)
    resp = session_client.patch(
        f"/api/runners/pods/{new_pod.id}/",
        {"is_default": True},
        format="json",
    )
    assert resp.status_code == status.HTTP_200_OK, resp.data
    assert resp.data["is_default"] is True
    old_default.refresh_from_db()
    assert old_default.is_default is False


# ---------------- soft-delete guards ----------------


@pytest.mark.unit
def test_delete_blocked_when_pod_has_runners(
    db, session_client, project, create_user
):
    pod = Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name=f"{project.identifier}_with-runner",
        created_by=create_user,
    )
    Runner.objects.create(
        owner=create_user,
        workspace=project.workspace,
        pod=pod,
        name="r-in-pod",
    )
    resp = session_client.delete(f"/api/runners/pods/{pod.id}/")
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["code"] == "pod_has_runners"


@pytest.mark.unit
def test_delete_blocked_when_pod_has_active_runs(
    db, session_client, project, create_user
):
    pod = Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name=f"{project.identifier}_with-active",
        created_by=create_user,
    )
    AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="x",
    )
    resp = session_client.delete(f"/api/runners/pods/{pod.id}/")
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["code"] == "pod_has_active_runs"


@pytest.mark.unit
def test_delete_blocked_for_default_pod(db, session_client, project):
    pod = Pod.default_for_project(project)
    resp = session_client.delete(f"/api/runners/pods/{pod.id}/")
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["code"] == "default_pod_undeletable"


@pytest.mark.unit
def test_delete_succeeds_for_non_default_when_guards_satisfied(
    db, session_client, project, create_user
):
    Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name=f"{project.identifier}_keepable",
        created_by=create_user,
        is_default=False,
    )
    second = Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name=f"{project.identifier}_doomed",
        created_by=create_user,
        is_default=False,
    )
    resp = session_client.delete(f"/api/runners/pods/{second.id}/")
    assert resp.status_code == status.HTTP_204_NO_CONTENT
    second.refresh_from_db()
    assert second.deleted_at is not None
