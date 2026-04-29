# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``GET /api/runners/projects/``.

Covers both auth modes (token-auth and session-auth) plus the
default-pod / pod-count fields in the response. See
``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§9.3 (CLI surface) and the cloud-side ProjectListEndpoint.
"""

from __future__ import annotations

import pytest
from django.urls import reverse

from pi_dash.db.models.project import Project
from pi_dash.db.models.workspace import Workspace, WorkspaceMember
from pi_dash.runner.models import MachineToken, Pod
from pi_dash.runner.services import tokens


def _make_token(workspace, user, title="laptop"):
    minted = tokens.mint_machine_token_secret()
    token = MachineToken.objects.create(
        workspace=workspace,
        created_by=user,
        title=title,
        secret_hash=minted.hashed,
        secret_fingerprint=minted.fingerprint,
    )
    return token, minted.raw


@pytest.mark.unit
def test_token_auth_returns_projects_in_token_workspace(
    db, api_client, workspace, project, create_user
):
    token, raw = _make_token(workspace, create_user)
    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {raw}",
        HTTP_X_TOKEN_ID=str(token.id),
    )
    url = reverse("project-list")
    resp = api_client.get(url)
    assert resp.status_code == 200, resp.content
    body = resp.json()
    identifiers = {row["identifier"] for row in body}
    assert project.identifier in identifiers
    # Default pod auto-created by the post_save(Project) signal.
    proj_row = next(r for r in body if r["identifier"] == project.identifier)
    assert proj_row["default_pod_id"] is not None
    assert proj_row["pod_count"] >= 1


@pytest.mark.unit
def test_token_auth_does_not_leak_other_workspace_projects(
    db, api_client, workspace, project, create_user
):
    """A token in workspace A must not see projects in workspace B."""
    other_ws = Workspace.objects.create(
        name="Other", owner=create_user, slug="other-ws"
    )
    WorkspaceMember.objects.create(workspace=other_ws, member=create_user, role=20)
    Project.objects.create(
        name="Hidden", identifier="HID", workspace=other_ws, created_by=create_user
    )

    token, raw = _make_token(workspace, create_user)
    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {raw}",
        HTTP_X_TOKEN_ID=str(token.id),
    )
    url = reverse("project-list")
    resp = api_client.get(url)
    assert resp.status_code == 200
    identifiers = {row["identifier"] for row in resp.json()}
    assert "HID" not in identifiers
    assert project.identifier in identifiers


@pytest.mark.unit
def test_token_auth_rejects_revoked_token(
    db, api_client, workspace, project, create_user
):
    token, raw = _make_token(workspace, create_user)
    token.revoked_at = __import__("django").utils.timezone.now()
    token.save(update_fields=["revoked_at"])
    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {raw}",
        HTTP_X_TOKEN_ID=str(token.id),
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
    token, raw = _make_token(workspace, create_user)
    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {raw}",
        HTTP_X_TOKEN_ID=str(token.id),
    )
    url = reverse("project-list")
    body = api_client.get(url).json()
    proj_row = next(r for r in body if r["identifier"] == project.identifier)
    # Default + beefy = 2.
    assert proj_row["pod_count"] == 2


@pytest.mark.unit
def test_response_embeds_pod_list_with_default_first(
    db, api_client, workspace, project, create_user
):
    """The TUI add-runner form needs the per-project pod list inline so it
    can render a cascaded picker without a second round-trip. The default
    pod must appear first so the picker pre-selects it."""
    Pod.objects.create(
        workspace=workspace,
        project=project,
        name=f"{project.identifier}_beefy",
        created_by=create_user,
    )
    token, raw = _make_token(workspace, create_user)
    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {raw}",
        HTTP_X_TOKEN_ID=str(token.id),
    )
    url = reverse("project-list")
    body = api_client.get(url).json()
    proj_row = next(r for r in body if r["identifier"] == project.identifier)

    pods = proj_row["pods"]
    assert isinstance(pods, list)
    assert len(pods) == 2
    # Default pod first; non-default after.
    assert pods[0]["is_default"] is True
    assert pods[0]["id"] == proj_row["default_pod_id"]
    assert any(p["name"].endswith("_beefy") and not p["is_default"] for p in pods)
    # Each pod entry carries the keys the TUI consumes.
    for p in pods:
        assert set(p.keys()) >= {"id", "name", "is_default"}


@pytest.mark.unit
def test_no_auth_returns_401(db, api_client, workspace, project):
    url = reverse("project-list")
    resp = api_client.get(url)
    assert resp.status_code == 401
