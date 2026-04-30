# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for ``POST /api/v1/runner/register-under-token/``.

Covers the project-scoped registration flow introduced in
``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§7.1: the daemon authenticates as a MachineToken, sends a runner name +
project identifier, and the cloud places the new runner in the project's
default pod (or an explicit named pod when supplied).
"""

from __future__ import annotations

import pytest
from django.urls import reverse

from pi_dash.db.models.project import Project
from pi_dash.db.models.workspace import Workspace, WorkspaceMember
from pi_dash.runner.models import MachineToken, Pod, Runner
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


def _auth(client, token, secret):
    client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {secret}",
        HTTP_X_TOKEN_ID=str(token.id),
    )


@pytest.mark.unit
def test_registers_under_default_pod_when_pod_omitted(
    db, api_client, workspace, project, create_user
):
    token, raw = _make_token(workspace, create_user)
    _auth(api_client, token, raw)
    url = reverse("runner:register-under-token")
    resp = api_client.post(
        url,
        {
            "name": "laptop-main",
            "project": project.identifier,
            "os": "linux",
            "arch": "x86_64",
            "version": "0.1.1",
            "protocol_version": 2,
        },
        format="json",
    )
    assert resp.status_code == 201, resp.content
    body = resp.json()
    runner = Runner.objects.get(id=body["runner_id"])
    default_pod = Pod.default_for_project(project)
    assert runner.pod_id == default_pod.id
    assert runner.machine_token_id == token.id
    assert body["pod_id"] == str(default_pod.id)


@pytest.mark.unit
def test_registers_under_explicit_pod_when_named(
    db, api_client, workspace, project, create_user
):
    token, raw = _make_token(workspace, create_user)
    # Pre-create a non-default pod the user wants this runner to land in.
    beefy = Pod.objects.create(
        workspace=workspace,
        project=project,
        name=f"{project.identifier}_beefy",
        created_by=create_user,
        is_default=False,
    )
    _auth(api_client, token, raw)
    url = reverse("runner:register-under-token")
    resp = api_client.post(
        url,
        {
            "name": "laptop-perf",
            "project": project.identifier,
            "pod": beefy.name,
            "os": "linux",
            "arch": "x86_64",
            "version": "0.1.1",
        },
        format="json",
    )
    assert resp.status_code == 201, resp.content
    body = resp.json()
    runner = Runner.objects.get(id=body["runner_id"])
    assert runner.pod_id == beefy.id


@pytest.mark.unit
def test_pod_can_be_passed_as_bare_suffix(
    db, api_client, workspace, project, create_user
):
    """Convenience: ``--pod beefy`` should match ``TEST_beefy`` if the
    user forgot the project prefix.
    """
    token, raw = _make_token(workspace, create_user)
    beefy = Pod.objects.create(
        workspace=workspace,
        project=project,
        name=f"{project.identifier}_beefy",
        created_by=create_user,
        is_default=False,
    )
    _auth(api_client, token, raw)
    url = reverse("runner:register-under-token")
    resp = api_client.post(
        url,
        {
            "name": "laptop-perf",
            "project": project.identifier,
            "pod": "beefy",  # bare suffix
        },
        format="json",
    )
    assert resp.status_code == 201, resp.content
    runner = Runner.objects.get(id=resp.json()["runner_id"])
    assert runner.pod_id == beefy.id


@pytest.mark.unit
def test_missing_project_returns_400(
    db, api_client, workspace, create_user
):
    token, raw = _make_token(workspace, create_user)
    _auth(api_client, token, raw)
    url = reverse("runner:register-under-token")
    resp = api_client.post(url, {"name": "laptop-main"}, format="json")
    assert resp.status_code == 400
    assert "project is required" in resp.json()["error"]


@pytest.mark.unit
def test_unknown_project_returns_404(
    db, api_client, workspace, project, create_user
):
    token, raw = _make_token(workspace, create_user)
    _auth(api_client, token, raw)
    url = reverse("runner:register-under-token")
    resp = api_client.post(
        url,
        {"name": "laptop-main", "project": "DOES_NOT_EXIST"},
        format="json",
    )
    assert resp.status_code == 404


@pytest.mark.unit
def test_cross_workspace_project_rejected(
    db, api_client, workspace, project, create_user
):
    """A token in workspace A cannot register a runner against a project
    in workspace B, even if the user owns both.
    """
    other_ws = Workspace.objects.create(
        name="Other", owner=create_user, slug="other-ws"
    )
    WorkspaceMember.objects.create(workspace=other_ws, member=create_user, role=20)
    other_project = Project.objects.create(
        name="Other Project",
        identifier="OTHER",
        workspace=other_ws,
        created_by=create_user,
    )

    token, raw = _make_token(workspace, create_user)
    _auth(api_client, token, raw)
    url = reverse("runner:register-under-token")
    resp = api_client.post(
        url,
        {"name": "laptop-main", "project": other_project.identifier},
        format="json",
    )
    assert resp.status_code == 404


@pytest.mark.unit
def test_soft_deleted_pod_rejected(
    db, api_client, workspace, project, create_user
):
    from django.utils import timezone

    token, raw = _make_token(workspace, create_user)
    dead_pod = Pod.objects.create(
        workspace=workspace,
        project=project,
        name=f"{project.identifier}_dead",
        created_by=create_user,
        deleted_at=timezone.now(),
    )
    _auth(api_client, token, raw)
    url = reverse("runner:register-under-token")
    resp = api_client.post(
        url,
        {
            "name": "laptop-main",
            "project": project.identifier,
            "pod": dead_pod.name,
        },
        format="json",
    )
    assert resp.status_code == 404


@pytest.mark.unit
def test_response_carries_pod_id_and_no_credential_secret(
    db, api_client, workspace, project, create_user
):
    """Token-auth runners don't carry a per-runner secret. The response
    deliberately omits ``credential_secret`` (would be wasted bytes the
    daemon ignores) and includes ``pod_id`` so the daemon can stamp it
    in ``config.toml`` for diagnostics.
    """
    token, raw = _make_token(workspace, create_user)
    _auth(api_client, token, raw)
    url = reverse("runner:register-under-token")
    resp = api_client.post(
        url,
        {"name": "laptop-main", "project": project.identifier},
        format="json",
    )
    body = resp.json()
    assert "credential_secret" not in body
    assert body["pod_id"] == str(Pod.default_for_project(project).id)
