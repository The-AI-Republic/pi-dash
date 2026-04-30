# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``GET /api/runners/projects/``.

Covers both auth modes — connection bearer (daemon) and session
(cloud UI) — plus default-pod / pod-count fields in the response.
"""

from __future__ import annotations

import pytest
from django.urls import reverse
from django.utils import timezone

from pi_dash.db.models.project import Project
from pi_dash.db.models.workspace import Workspace, WorkspaceMember
from pi_dash.runner.models import Connection, Pod
from pi_dash.runner.services import tokens


def _make_connection(workspace, user, name="connection_test"):
    secret = tokens.mint_connection_secret()
    connection = Connection.objects.create(
        workspace=workspace,
        created_by=user,
        name=name,
        secret_hash=secret.hashed,
        secret_fingerprint=secret.fingerprint,
        enrolled_at=timezone.now(),
    )
    return connection, secret.raw


@pytest.mark.unit
def test_connection_auth_returns_projects_in_workspace(
    db, api_client, workspace, project, create_user
):
    connection, raw = _make_connection(workspace, create_user)
    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {raw}",
        HTTP_X_CONNECTION_ID=str(connection.id),
    )
    url = reverse("project-list")
    resp = api_client.get(url)
    assert resp.status_code == 200, resp.content
    body = resp.json()
    identifiers = {row["identifier"] for row in body}
    assert project.identifier in identifiers
    proj_row = next(r for r in body if r["identifier"] == project.identifier)
    assert proj_row["default_pod_id"] is not None
    assert proj_row["pod_count"] >= 1


@pytest.mark.unit
def test_connection_auth_does_not_leak_other_workspace_projects(
    db, api_client, workspace, project, create_user
):
    other_ws = Workspace.objects.create(
        name="Other", owner=create_user, slug="other-ws"
    )
    WorkspaceMember.objects.create(workspace=other_ws, member=create_user, role=20)
    Project.objects.create(
        name="Hidden", identifier="HID", workspace=other_ws, created_by=create_user
    )

    connection, raw = _make_connection(workspace, create_user)
    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {raw}",
        HTTP_X_CONNECTION_ID=str(connection.id),
    )
    url = reverse("project-list")
    resp = api_client.get(url)
    assert resp.status_code == 200
    identifiers = {row["identifier"] for row in resp.json()}
    assert "HID" not in identifiers
    assert project.identifier in identifiers


@pytest.mark.unit
def test_connection_auth_rejects_revoked_connection(
    db, api_client, workspace, project, create_user
):
    connection, raw = _make_connection(workspace, create_user)
    connection.revoked_at = timezone.now()
    connection.save(update_fields=["revoked_at"])
    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {raw}",
        HTTP_X_CONNECTION_ID=str(connection.id),
    )
    url = reverse("project-list")
    resp = api_client.get(url)
    assert resp.status_code == 401


@pytest.mark.unit
def test_pod_count_reflects_user_created_pods(
    db, api_client, workspace, project, create_user
):
    Pod.objects.create(
        workspace=workspace,
        project=project,
        name=f"{project.identifier}_beefy",
        created_by=create_user,
    )
    connection, raw = _make_connection(workspace, create_user)
    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {raw}",
        HTTP_X_CONNECTION_ID=str(connection.id),
    )
    url = reverse("project-list")
    body = api_client.get(url).json()
    proj_row = next(r for r in body if r["identifier"] == project.identifier)
    assert proj_row["pod_count"] == 2


@pytest.mark.unit
def test_response_embeds_pod_list_with_default_first(
    db, api_client, workspace, project, create_user
):
    Pod.objects.create(
        workspace=workspace,
        project=project,
        name=f"{project.identifier}_beefy",
        created_by=create_user,
    )
    connection, raw = _make_connection(workspace, create_user)
    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {raw}",
        HTTP_X_CONNECTION_ID=str(connection.id),
    )
    url = reverse("project-list")
    body = api_client.get(url).json()
    proj_row = next(r for r in body if r["identifier"] == project.identifier)

    pods = proj_row["pods"]
    assert isinstance(pods, list)
    assert len(pods) == 2
    assert pods[0]["is_default"] is True
    assert pods[0]["id"] == proj_row["default_pod_id"]
    assert any(p["name"].endswith("_beefy") and not p["is_default"] for p in pods)
    for p in pods:
        assert set(p.keys()) >= {"id", "name", "is_default"}


@pytest.mark.unit
def test_no_auth_returns_401(db, api_client, workspace, project):
    url = reverse("project-list")
    resp = api_client.get(url)
    assert resp.status_code == 401
