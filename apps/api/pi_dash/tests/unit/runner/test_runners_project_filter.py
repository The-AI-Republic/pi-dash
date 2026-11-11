# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Project-scoped filtering for the AI Workers panel endpoints.

The workspace-wide runners/runs/approvals/chat lists gain an optional
``?project=<uuid>`` filter that narrows to every row whose pod belongs to the
project (a project owns several pods). These tests pin that behaviour and, for
runs, that the private-runner visibility rule survives the new filter.

``GET /api/runners/`` additionally accepts ``?project=`` alone (no
``?workspace=``) — like the pod list endpoint, the project wins and its
workspace is used — which is how the old web's Runs-on picker lists runners.
"""

from __future__ import annotations

from unittest.mock import patch
from uuid import uuid4

import pytest
from django.utils import timezone
from rest_framework import status

from pi_dash.db.models import User, Workspace, WorkspaceMember
from pi_dash.db.models.project import Project
from pi_dash.db.models.state import State
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
# GET /api/runners/ — ``?project=`` alone (Runs-on picker parity with pods)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_runners_project_alone_lists_project_runners(db, session_client, workspace, project, second_project):
    """``?project=`` without ``?workspace=`` lists that project's runners."""
    p1 = Pod.default_for_project(project)
    p2 = Pod.default_for_project(second_project)
    _make_runner(workspace, p1, owner=workspace.owner, name="runner-1")
    _make_runner(workspace, p2, owner=workspace.owner, name="runner-2")

    resp = session_client.get("/api/runners/", {"project": str(project.id)})

    assert resp.status_code == status.HTTP_200_OK
    names = {r["name"] for r in resp.data}
    assert "runner-1" in names
    assert "runner-2" not in names


@pytest.mark.unit
def test_runners_project_alone_stays_owner_only(db, session_client, workspace, project):
    """Another member's runner is not listed when filtering by project alone."""
    other = _make_user()
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    p1 = Pod.default_for_project(project)
    _make_runner(workspace, p1, owner=workspace.owner, name="own-runner")
    _make_runner(workspace, p1, owner=other, name="member-runner")

    resp = session_client.get("/api/runners/", {"project": str(project.id)})

    assert resp.status_code == status.HTTP_200_OK
    names = {r["name"] for r in resp.data}
    assert "own-runner" in names
    assert "member-runner" not in names


@pytest.mark.unit
def test_runners_project_alone_non_member_forbidden(db, api_client, workspace, project):
    outsider = _make_user()
    api_client.force_authenticate(user=outsider)

    resp = api_client.get("/api/runners/", {"project": str(project.id)})

    assert resp.status_code == status.HTTP_403_FORBIDDEN
    assert resp.data == {"error": "forbidden"}


@pytest.mark.unit
def test_runners_project_alone_unknown_project_404(db, session_client):
    resp = session_client.get("/api/runners/", {"project": str(uuid4())})

    assert resp.status_code == status.HTTP_404_NOT_FOUND
    assert resp.data == {"error": "project not found"}


@pytest.mark.unit
def test_runners_neither_param_400(db, session_client):
    resp = session_client.get("/api/runners/")

    assert resp.status_code == status.HTTP_400_BAD_REQUEST
    assert resp.data == {"error": "project or workspace is required"}


@pytest.mark.unit
def test_runners_workspace_alone_unchanged(db, session_client, workspace, project, second_project):
    """``?workspace=`` alone still lists every runner in the workspace."""
    p1 = Pod.default_for_project(project)
    p2 = Pod.default_for_project(second_project)
    _make_runner(workspace, p1, owner=workspace.owner, name="runner-1")
    _make_runner(workspace, p2, owner=workspace.owner, name="runner-2")

    resp = session_client.get("/api/runners/", {"workspace": str(workspace.id)})

    assert resp.status_code == status.HTTP_200_OK
    names = {r["name"] for r in resp.data}
    assert "runner-1" in names
    assert "runner-2" in names


@pytest.mark.unit
def test_runners_project_wins_over_mismatched_workspace(db, session_client, workspace, project, create_user):
    """With both params, the project's workspace scopes the list — a
    mismatched ``?workspace=`` cannot widen what is visible."""
    other_workspace = Workspace.objects.create(name="Other Workspace", owner=create_user, slug="other-workspace")
    other_project = Project.objects.create(
        name="Other Project",
        identifier="OTHER",
        workspace=other_workspace,
        created_by=create_user,
    )
    p1 = Pod.default_for_project(project)
    p2 = Pod.default_for_project(other_project)
    _make_runner(workspace, p1, owner=workspace.owner, name="runner-in-project")
    _make_runner(other_workspace, p2, owner=create_user, name="runner-in-other")

    resp = session_client.get(
        "/api/runners/",
        {"project": str(project.id), "workspace": str(other_workspace.id)},
    )

    assert resp.status_code == status.HTTP_200_OK
    names = {r["name"] for r in resp.data}
    assert "runner-in-project" in names
    assert "runner-in-other" not in names


@pytest.mark.unit
def test_runners_project_alone_with_assignable_for(db, session_client, workspace, project, create_user):
    """``?project=`` + ``?assignable_for=`` still carries the eligibility fields."""
    from crum import impersonate

    from pi_dash.db.models.issue import Issue

    p1 = Pod.default_for_project(project)
    runner = _make_runner(workspace, p1, owner=create_user, name="gpu-box")
    issue_state = State.objects.create(
        name="Backlog",
        group="backlog",
        workspace=workspace,
        project=project,
        sequence=1.0,
        default=True,
    )
    with impersonate(create_user):
        issue = Issue.objects.create(
            name="An issue",
            project=project,
            workspace=workspace,
            state=issue_state,
            created_by=create_user,
            assigned_pod=p1,
            assigned_runner=runner,
        )

    resp = session_client.get(
        "/api/runners/",
        {"project": str(project.id), "assignable_for": str(issue.id)},
    )

    assert resp.status_code == status.HTTP_200_OK
    row = next(r for r in resp.data if str(r["id"]) == str(runner.id))
    assert row["assignable"] is True
    assert row["not_assignable_code"] == ""
    assert row["is_busy"] is False
    assert row["queue_depth"] == 0


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

    resp = session_client.get(
        "/api/runners/chat/sessions/", {"workspace": str(workspace.id), "project": str(project.id)}
    )

    assert resp.status_code == status.HTTP_200_OK
    ids = {s["id"] for s in resp.data}
    assert str(s1.id) in ids
    assert str(s2.id) not in ids
