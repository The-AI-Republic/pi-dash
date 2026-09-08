# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Project-scoped filtering for the AI Workers panel endpoints.

The workspace-wide runners/runs/approvals/chat lists gain an optional
``?project=<uuid>`` filter that narrows to every row whose pod belongs to the
project (a project owns several pods). These tests pin that behaviour and, for
runs, that the private-runner visibility rule survives the new filter.
"""

from __future__ import annotations

from unittest.mock import patch
from uuid import uuid4

import pytest
from django.utils import timezone
from rest_framework import status

from pi_dash.db.models import User
from pi_dash.db.models.project import Project
from pi_dash.runner.models import (
    AgentChatSession,
    AgentRun,
    ApprovalKind,
    ApprovalRequest,
    ApprovalStatus,
    Pod,
    Runner,
    RunnerStatus,
)


@pytest.fixture
def second_project(workspace, create_user):
    """A second project in the same workspace, so ``project`` (identifier DEF)
    and this one each own a distinct default pod."""
    return Project.objects.create(
        name="Second Project",
        identifier="SECOND",
        workspace=workspace,
        created_by=create_user,
    )


@pytest.fixture(autouse=True)
def _on_commit_immediate():
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


def _make_user() -> User:
    user = User.objects.create(
        email=f"o-{uuid4().hex[:8]}@example.com",
        username=f"o_{uuid4().hex[:8]}",
    )
    user.set_password("pw")
    user.save()
    return user


def _make_runner(workspace, pod, owner, name) -> Runner:
    return Runner.objects.create(
        owner=owner,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


# ---------------------------------------------------------------------------
# GET /api/runners/runs/?project=
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_runs_project_filter_scopes_to_project_pods(db, session_client, workspace, project, second_project):
    p1 = Pod.default_for_project(project)
    p2 = Pod.default_for_project(second_project)
    AgentRun.objects.create(workspace=workspace, created_by=workspace.owner, pod=p1, prompt="run-in-project")
    AgentRun.objects.create(workspace=workspace, created_by=workspace.owner, pod=p2, prompt="run-in-second")

    resp = session_client.get("/api/runners/runs/", {"project": str(project.id)})

    assert resp.status_code == status.HTTP_200_OK
    prompts = [r["prompt"] for r in resp.data["results"]]
    assert "run-in-project" in prompts
    assert "run-in-second" not in prompts


@pytest.mark.unit
def test_runs_project_filter_preserves_private_runner_rule(db, session_client, workspace, project):
    """A run executing on another user's PRIVATE runner stays hidden even when
    the caller is a workspace admin and the run's pod is in the queried
    project — the project filter must AND-combine with the visibility gate, not
    widen it."""
    other = _make_user()
    p1 = Pod.default_for_project(project)
    private_runner = _make_runner(workspace, p1, owner=other, name="other-private")
    AgentRun.objects.create(
        workspace=workspace,
        created_by=other,
        runner=private_runner,
        pod=p1,
        prompt="hidden-private-run",
    )

    resp = session_client.get("/api/runners/runs/", {"project": str(project.id)})

    assert resp.status_code == status.HTTP_200_OK
    prompts = [r["prompt"] for r in resp.data["results"]]
    assert "hidden-private-run" not in prompts


# ---------------------------------------------------------------------------
# GET /api/runners/approvals/?project=
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_approvals_project_filter(db, session_client, workspace, project, second_project):
    p1 = Pod.default_for_project(project)
    p2 = Pod.default_for_project(second_project)
    run1 = AgentRun.objects.create(workspace=workspace, created_by=workspace.owner, pod=p1, prompt="r1")
    run2 = AgentRun.objects.create(workspace=workspace, created_by=workspace.owner, pod=p2, prompt="r2")
    ApprovalRequest.objects.create(agent_run=run1, kind=ApprovalKind.COMMAND_EXECUTION, status=ApprovalStatus.PENDING)
    ApprovalRequest.objects.create(agent_run=run2, kind=ApprovalKind.COMMAND_EXECUTION, status=ApprovalStatus.PENDING)

    scoped = session_client.get("/api/runners/approvals/", {"project": str(project.id)})
    unscoped = session_client.get("/api/runners/approvals/")

    assert scoped.status_code == status.HTTP_200_OK
    # Only the approval whose run's pod belongs to ``project`` survives.
    assert len(scoped.data) == 1
    assert len(unscoped.data) == 2


# ---------------------------------------------------------------------------
# GET /api/runners/?project=
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_runners_project_filter(db, session_client, workspace, project, second_project):
    p1 = Pod.default_for_project(project)
    p2 = Pod.default_for_project(second_project)
    _make_runner(workspace, p1, owner=workspace.owner, name="runner-1")
    _make_runner(workspace, p2, owner=workspace.owner, name="runner-2")

    resp = session_client.get("/api/runners/", {"workspace": str(workspace.id), "project": str(project.id)})

    assert resp.status_code == status.HTTP_200_OK
    names = {r["name"] for r in resp.data}
    assert "runner-1" in names
    assert "runner-2" not in names


# ---------------------------------------------------------------------------
# GET /api/runners/chat/sessions/?project=
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_chat_sessions_project_filter(db, session_client, workspace, project, second_project):
    p1 = Pod.default_for_project(project)
    p2 = Pod.default_for_project(second_project)
    r1 = _make_runner(workspace, p1, owner=workspace.owner, name="cr-1")
    r2 = _make_runner(workspace, p2, owner=workspace.owner, name="cr-2")
    s1 = AgentChatSession.objects.create(workspace=workspace, runner=r1, created_by=workspace.owner, pod=p1)
    s2 = AgentChatSession.objects.create(workspace=workspace, runner=r2, created_by=workspace.owner, pod=p2)

    resp = session_client.get("/api/runners/chat/sessions/", {"workspace": str(workspace.id), "project": str(project.id)})

    assert resp.status_code == status.HTTP_200_OK
    ids = {s["id"] for s in resp.data}
    assert str(s1.id) in ids
    assert str(s2.id) not in ids
