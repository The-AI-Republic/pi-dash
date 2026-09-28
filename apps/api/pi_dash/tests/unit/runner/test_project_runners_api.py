# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``GET /api/v1/workspaces/<slug>/projects/<project>/runners/``.

The token-auth (X-Api-Key) runner list the CLI's ``pidash project runners``
calls. It must show exactly what the session-auth AI Workers panel shows for
the same user: both delegate to
``pi_dash.runner.services.runner_directory.project_runners_queryset``, so the
rules pinned here — workspace membership, private-runner visibility, the
project and pod narrowing, and the desktop-bundled exclusion — hold for the
web surface too.
"""

from __future__ import annotations

import uuid

import pytest
from django.urls import reverse
from django.utils import timezone

from pi_dash.db.models import APIToken, User, WorkspaceMember
from pi_dash.db.models.project import Project
from pi_dash.runner.models import Pod, Runner, RunnerProvisioning, RunnerStatus


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def second_project(workspace, create_user):
    return Project.objects.create(
        name="Second Project",
        identifier="SECOND",
        workspace=workspace,
        created_by=create_user,
    )


def _make_user(tag: str) -> User:
    user = User.objects.create_user(email=f"{tag}@example.com", username=tag)
    user.set_password("pw")
    user.save()
    return user


def _make_runner(owner, workspace, pod, name, **kwargs) -> Runner:
    return Runner.objects.create(
        owner=owner,
        workspace=workspace,
        pod=pod,
        name=name,
        status=kwargs.pop("status", RunnerStatus.ONLINE),
        last_heartbeat_at=kwargs.pop("last_heartbeat_at", timezone.now()),
        **kwargs,
    )


def _url(workspace, project_ref) -> str:
    return reverse(
        "api-project-runners",
        kwargs={"slug": workspace.slug, "project_id": str(project_ref)},
    )


@pytest.mark.unit
def test_lists_project_runners_with_status_and_heartbeat(
    db, api_key_client, create_user, workspace, project, pod
):
    r = _make_runner(create_user, workspace, pod, "mine")
    resp = api_key_client.get(_url(workspace, project.id))
    assert resp.status_code == 200
    assert [row["id"] for row in resp.data] == [str(r.id)]
    row = resp.data[0]
    assert row["name"] == "mine"
    assert row["status"] == RunnerStatus.ONLINE
    assert row["last_heartbeat_at"] is not None
    # PrimaryKeyRelatedField yields a UUID instance pre-render.
    assert str(row["pod"]) == str(pod.id)


@pytest.mark.unit
def test_resolves_project_by_identifier(db, api_key_client, create_user, workspace, project, pod):
    r = _make_runner(create_user, workspace, pod, "by-identifier")
    # Lower-case on purpose: Project.resolve normalises identifiers to upper.
    resp = api_key_client.get(_url(workspace, project.identifier.lower()))
    assert resp.status_code == 200
    assert [row["id"] for row in resp.data] == [str(r.id)]


@pytest.mark.unit
def test_unknown_project_is_404(db, api_key_client, workspace):
    assert api_key_client.get(_url(workspace, "NOPE")).status_code == 404
    assert api_key_client.get(_url(workspace, uuid.uuid4())).status_code == 404


@pytest.mark.unit
def test_project_in_another_workspace_is_404(db, api_key_client, create_user, workspace, project):
    """A valid project UUID under the wrong workspace slug must not resolve."""
    from pi_dash.db.models import Workspace

    other_ws = Workspace.objects.create(name="Other", owner=create_user, slug="other-ws")
    WorkspaceMember.objects.create(workspace=other_ws, member=create_user, role=20)
    resp = api_key_client.get(_url(other_ws, project.id))
    assert resp.status_code == 404


@pytest.mark.unit
def test_non_member_gets_403(db, api_client, create_user, workspace, project, pod):
    _make_runner(create_user, workspace, pod, "members-only")
    outsider = _make_user("outsider-245")
    token = APIToken.objects.create(user=outsider, label="outsider", token="outsider-tok-245")
    api_client.credentials(HTTP_X_API_KEY=token.token)
    resp = api_client.get(_url(workspace, project.id))
    assert resp.status_code == 403


@pytest.mark.unit
def test_unauthenticated_is_401_or_403(db, api_client, workspace, project):
    resp = api_client.get(_url(workspace, project.id))
    assert resp.status_code in (401, 403)


@pytest.mark.unit
def test_other_users_private_runners_are_hidden(
    db, api_key_client, create_user, workspace, project, pod
):
    """Even a workspace admin never sees another member's private runners."""
    other = _make_user("other-private-245")
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    _make_runner(other, workspace, pod, "other-private")
    mine = _make_runner(create_user, workspace, pod, "mine")
    resp = api_key_client.get(_url(workspace, project.id))
    assert resp.status_code == 200
    assert [row["id"] for row in resp.data] == [str(mine.id)]


@pytest.mark.unit
def test_scopes_to_the_named_projects_pods(
    db, api_key_client, create_user, workspace, project, pod, second_project
):
    second_pod = Pod.default_for_project(second_project)
    _make_runner(create_user, workspace, second_pod, "in-second")
    in_project = _make_runner(create_user, workspace, pod, "in-project")
    resp = api_key_client.get(_url(workspace, project.id))
    assert resp.status_code == 200
    assert [row["id"] for row in resp.data] == [str(in_project.id)]


@pytest.mark.unit
def test_pod_filter_narrows_within_the_project(
    db, api_key_client, create_user, workspace, project, pod
):
    extra_pod = Pod.objects.create(
        workspace=workspace, project=project, name="tier-2", created_by=create_user
    )
    _make_runner(create_user, workspace, pod, "default-pod")
    in_extra = _make_runner(create_user, workspace, extra_pod, "extra-pod")
    resp = api_key_client.get(_url(workspace, project.id), {"pod": str(extra_pod.id)})
    assert resp.status_code == 200
    assert [row["id"] for row in resp.data] == [str(in_extra.id)]


@pytest.mark.unit
def test_bundled_runners_hidden_unless_asked_for(
    db, api_key_client, create_user, workspace, project, pod
):
    bundled = _make_runner(
        create_user,
        workspace,
        pod,
        "desktop-bundled",
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
    )
    plain = _make_runner(create_user, workspace, pod, "plain")

    resp = api_key_client.get(_url(workspace, project.id))
    assert [row["id"] for row in resp.data] == [str(plain.id)]

    resp = api_key_client.get(_url(workspace, project.id), {"include_bundled": "true"})
    ids = {row["id"] for row in resp.data}
    assert ids == {str(plain.id), str(bundled.id)}


@pytest.mark.unit
def test_response_carries_no_secret_material(db, api_key_client, create_user, workspace, project, pod):
    _make_runner(create_user, workspace, pod, "no-secrets")
    resp = api_key_client.get(_url(workspace, project.id))
    assert resp.status_code == 200
    keys = set(resp.data[0].keys())
    assert not any("token" in k or "secret" in k for k in keys)
