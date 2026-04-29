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
    """Free-form runs (no work_item) are scoped by creator. A run created
    by another user with no link back to the caller stays invisible.
    """
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
# GET /api/runners/runs/ — broadened "involved with" scope.
#
# Runs created by the system bot for periodic ticks (``triggered_by=tick``)
# carry ``created_by = agent system user`` per
# ``orchestration/scheduling._resolve_creator_for_trigger``. Filtering only
# by ``created_by = request.user`` would hide every tick-driven run from
# the human who actually owns the issue. The list endpoint therefore
# surfaces any run whose ``work_item`` was created by, or is assigned to,
# the caller — in addition to runs the caller created directly.
# ---------------------------------------------------------------------------


def _make_other_user():
    user = User.objects.create(
        email=f"o-{uuid4().hex[:8]}@example.com",
        username=f"o_{uuid4().hex[:8]}",
    )
    user.set_password("pw")
    user.save()
    return user


def _make_issue(workspace, *, created_by, assignees=()):
    """Create a minimal Issue inside the workspace's default-ish project.

    ``IssueAssignee`` (the M2M through model) extends ``ProjectBaseModel``
    which has a non-null ``created_by`` — so we must create rows
    explicitly rather than via ``issue.assignees.set(...)``.
    """
    from crum import impersonate

    from pi_dash.db.models import Issue, Project, State
    from pi_dash.db.models.issue import IssueAssignee

    with impersonate(created_by):
        project = Project.objects.create(
            name=f"P-{uuid4().hex[:6]}",
            identifier=f"P{uuid4().hex[:4].upper()}",
            workspace=workspace,
            created_by=created_by,
        )
        started = State.objects.create(name="In Progress", project=project, group="started")
        issue = Issue.objects.create(
            name="task",
            workspace=workspace,
            project=project,
            state=started,
            created_by=created_by,
        )
        for assignee in assignees:
            IssueAssignee.objects.create(
                issue=issue,
                assignee=assignee,
                workspace=workspace,
                project=project,
                created_by=created_by,
            )
    # The state-transition signal may auto-create an initial dispatch run;
    # delete it so each test controls exactly which runs exist.
    AgentRun.objects.filter(work_item=issue).delete()
    return issue


@pytest.mark.unit
def test_get_runs_includes_tick_runs_on_issue_user_created(db, session_client, workspace):
    """An issue I created has a tick-driven run authored by the bot.
    The run must appear in my list even though I didn't create it.
    """
    other = _make_other_user()  # stand-in for the bot
    issue = _make_issue(workspace, created_by=workspace.owner)
    AgentRun.objects.create(
        workspace=workspace,
        created_by=other,
        pod=Pod.default_for_workspace(workspace),
        work_item=issue,
        prompt="tick run on my issue",
    )
    resp = session_client.get("/api/runners/runs/")
    assert resp.status_code == status.HTTP_200_OK
    assert "tick run on my issue" in [r["prompt"] for r in resp.data]


@pytest.mark.unit
def test_get_runs_includes_tick_runs_on_issue_user_assigned(db, session_client, workspace):
    """An issue I'm assigned to (but didn't create) has a tick-driven run
    authored by the bot. The run must still appear in my list.
    """
    other = _make_other_user()
    issue = _make_issue(workspace, created_by=other, assignees=[workspace.owner])
    AgentRun.objects.create(
        workspace=workspace,
        created_by=other,
        pod=Pod.default_for_workspace(workspace),
        work_item=issue,
        prompt="tick run on assigned issue",
    )
    resp = session_client.get("/api/runners/runs/")
    assert resp.status_code == status.HTTP_200_OK
    assert "tick run on assigned issue" in [r["prompt"] for r in resp.data]


@pytest.mark.unit
def test_get_runs_excludes_runs_on_unrelated_issues(db, session_client, workspace):
    """Negative case: an issue I neither created nor am assigned to, with
    a run also not created by me. Stays invisible.
    """
    other = _make_other_user()
    issue = _make_issue(workspace, created_by=other)
    AgentRun.objects.create(
        workspace=workspace,
        created_by=other,
        pod=Pod.default_for_workspace(workspace),
        work_item=issue,
        prompt="run on unrelated issue",
    )
    resp = session_client.get("/api/runners/runs/")
    assert resp.status_code == status.HTTP_200_OK
    assert "run on unrelated issue" not in [r["prompt"] for r in resp.data]


@pytest.mark.unit
def test_get_runs_does_not_duplicate_when_user_satisfies_multiple_clauses(
    db, session_client, workspace
):
    """Caller created the issue AND is assigned to it AND created the run.
    The run still appears exactly once.
    """
    issue = _make_issue(workspace, created_by=workspace.owner, assignees=[workspace.owner])
    AgentRun.objects.create(
        workspace=workspace,
        created_by=workspace.owner,
        pod=Pod.default_for_workspace(workspace),
        work_item=issue,
        prompt="multi-match run",
    )
    resp = session_client.get("/api/runners/runs/")
    assert resp.status_code == status.HTTP_200_OK
    prompts = [r["prompt"] for r in resp.data]
    assert prompts.count("multi-match run") == 1


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


# ---------------------------------------------------------------------------
# Comment & Run endpoint
#
# POST /api/runners/runs/ with ``triggered_by="comment_and_run"`` reuses the
# continuation pipeline (parent resolution, runner pinning, drain) and
# resets the issue's ``IssueAgentSchedule``. See
# ``.ai_design/issue_ticking_system/design.md`` §4.6.
# ---------------------------------------------------------------------------


def _make_in_progress_issue_with_paused_run(workspace):
    """Build an issue in the literal ``In Progress`` state with a prior
    ``PAUSED_AWAITING_INPUT`` run — the prerequisites for ``Comment & Run``."""
    from datetime import timedelta

    from crum import impersonate
    from django.utils import timezone

    from pi_dash.db.models import Issue, Project, State
    from pi_dash.runner.models import Runner, RunnerStatus

    pod = Pod.default_for_workspace(workspace)
    runner = Runner.objects.create(
        owner=workspace.owner,
        workspace=workspace,
        pod=pod,
        name="carun",
        credential_hash="h",
        credential_fingerprint="f" * 12,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )
    with impersonate(workspace.owner):
        project = Project.objects.create(
            name="CARun",
            identifier="CARN",
            workspace=workspace,
            created_by=workspace.owner,
        )
        in_progress = State.objects.create(
            name="In Progress", project=project, group="started"
        )
        State.objects.create(name="Todo", project=project, group="unstarted")
        issue = Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=in_progress,
            created_by=workspace.owner,
        )
    # The state-transition signal auto-creates an immediate-dispatch run
    # (and the matcher may have flipped it to ASSIGNED). Delete it so the
    # test's PAUSED parent is the only prior run, with no active-run noise.
    AgentRun.objects.filter(work_item=issue).delete()
    parent = AgentRun.objects.create(
        workspace=workspace,
        owner=workspace.owner,
        pod=pod,
        work_item=issue,
        runner=runner,
        thread_id="sess_xyz",
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
        prompt="prior",
        started_at=timezone.now() - timedelta(minutes=5),
    )
    return issue, parent


@pytest.mark.unit
def test_comment_and_run_creates_continuation_with_parent_link(
    db, session_client, workspace
):
    """Happy path: prior PAUSED run exists, ``triggered_by`` routes to the
    continuation pipeline, ``parent_run`` is wired correctly."""
    issue, parent = _make_in_progress_issue_with_paused_run(workspace)

    resp = session_client.post(
        "/api/runners/runs/",
        {
            "workspace": str(workspace.id),
            "work_item": str(issue.id),
            "triggered_by": "comment_and_run",
        },
        format="json",
    )
    assert resp.status_code == status.HTTP_201_CREATED, resp.data
    run = AgentRun.objects.get(id=resp.data["id"])
    assert run.parent_run_id == parent.id
    # Status may be QUEUED or ASSIGNED — drain may have flipped it on commit
    # (the runner_for_workspace fixture is online and idle). The contract
    # is "continuation row created, linked to the prior run," not a
    # specific terminal status of the post-commit drain.
    assert run.status in (AgentRunStatus.QUEUED, AgentRunStatus.ASSIGNED)


@pytest.mark.unit
def test_comment_and_run_resets_schedule(db, session_client, workspace):
    """The endpoint must reset ``tick_count`` and bump ``next_run_at`` —
    that's what makes Comment & Run a fresh budget grant per §4.6 step 4."""
    from datetime import timedelta

    from django.utils import timezone

    from pi_dash.db.models.issue_agent_schedule import IssueAgentSchedule

    issue, _ = _make_in_progress_issue_with_paused_run(workspace)
    sched = IssueAgentSchedule.objects.get(issue=issue)
    sched.tick_count = 7
    stale = timezone.now() + timedelta(hours=2)
    sched.next_run_at = stale
    sched.save(update_fields=["tick_count", "next_run_at"])

    resp = session_client.post(
        "/api/runners/runs/",
        {
            "workspace": str(workspace.id),
            "work_item": str(issue.id),
            "triggered_by": "comment_and_run",
        },
        format="json",
    )
    assert resp.status_code == status.HTTP_201_CREATED
    sched.refresh_from_db()
    assert sched.tick_count == 0
    # next_run_at moved off the stale future timestamp toward NOW + interval.
    assert sched.next_run_at != stale


@pytest.mark.unit
def test_comment_and_run_requires_work_item(db, session_client, workspace):
    resp = session_client.post(
        "/api/runners/runs/",
        {"workspace": str(workspace.id), "triggered_by": "comment_and_run"},
        format="json",
    )
    assert resp.status_code == status.HTTP_400_BAD_REQUEST
    assert "work_item" in resp.data["error"]


@pytest.mark.unit
def test_comment_and_run_returns_409_when_no_prior_run(
    db, session_client, workspace
):
    """Continuation pipeline can't run with no prior — a Comment & Run
    on an issue that never had a run is a 409, not a silent no-op."""
    from crum import impersonate

    from pi_dash.db.models import Issue, Project, State

    with impersonate(workspace.owner):
        project = Project.objects.create(
            name="P2", identifier="P2", workspace=workspace,
            created_by=workspace.owner,
        )
        in_progress = State.objects.create(
            name="In Progress", project=project, group="started"
        )
        issue = Issue.objects.create(
            name="Task", workspace=workspace, project=project,
            state=in_progress, created_by=workspace.owner,
        )
    # Discard the single AgentRun the state-transition signal auto-created
    # so the endpoint sees no prior.
    AgentRun.objects.filter(work_item=issue).delete()

    resp = session_client.post(
        "/api/runners/runs/",
        {
            "workspace": str(workspace.id),
            "work_item": str(issue.id),
            "triggered_by": "comment_and_run",
        },
        format="json",
    )
    assert resp.status_code == status.HTTP_409_CONFLICT


@pytest.mark.unit
def test_comment_and_run_404_for_issue_in_other_workspace(
    db, api_client, workspace, second_workspace
):
    issue, _ = _make_in_progress_issue_with_paused_run(workspace)
    outsider = User.objects.create(
        email=f"o-{uuid4().hex[:8]}@example.com",
        username=f"o_{uuid4().hex[:8]}",
    )
    outsider.set_password("pw")
    outsider.save()
    api_client.force_authenticate(user=outsider)
    resp = api_client.post(
        "/api/runners/runs/",
        {
            "workspace": str(second_workspace.id),
            "work_item": str(issue.id),
            "triggered_by": "comment_and_run",
        },
        format="json",
    )
    # Outsider isn't a member of the issue's workspace — 404 (not 403)
    # so existence isn't confirmed across workspaces.
    assert resp.status_code == status.HTTP_404_NOT_FOUND


# ---------------------------------------------------------------------------
# X-Pi-Dash-Skip-Immediate-Dispatch header path
#
# Used by the Comment & Run flow on a Paused issue: the client wants to
# transition the issue back to In Progress without firing the state-
# transition signal's own immediate dispatch (Comment & Run owns the
# dispatch and would race the signal otherwise). See
# ``.ai_design/issue_ticking_system/design.md`` §4.5–§4.6.
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_skip_immediate_dispatch_header_suppresses_run_creation_on_state_change(
    db, session_client, workspace
):
    from crum import impersonate

    from pi_dash.db.models import Issue, Project, State
    from pi_dash.db.models.issue_agent_schedule import IssueAgentSchedule
    from pi_dash.prompting.seed import seed_default_template

    seed_default_template()
    with impersonate(workspace.owner):
        project = Project.objects.create(
            name="SkipDisp", identifier="SD", workspace=workspace,
            created_by=workspace.owner,
        )
        todo = State.objects.create(
            name="Todo", project=project, group="unstarted"
        )
        in_progress = State.objects.create(
            name="In Progress", project=project, group="started"
        )
        issue = Issue.objects.create(
            name="Task", workspace=workspace, project=project,
            state=todo, created_by=workspace.owner,
        )

    # PATCH state=In Progress with the skip header — signal must not
    # create an AgentRun, but must still arm the schedule.
    resp = session_client.patch(
        f"/api/workspaces/{workspace.slug}/projects/{project.id}/issues/{issue.id}/",
        {"state_id": str(in_progress.id)},
        format="json",
        HTTP_X_PI_DASH_SKIP_IMMEDIATE_DISPATCH="1",
    )
    assert resp.status_code == status.HTTP_204_NO_CONTENT
    assert AgentRun.objects.filter(work_item=issue).count() == 0
    # Schedule armed: the steady-state tick source is in place even
    # though dispatch was deferred to the caller.
    sched = IssueAgentSchedule.objects.get(issue=issue)
    assert sched.enabled is True


@pytest.mark.unit
def test_no_skip_header_creates_run_on_state_change(
    db, session_client, workspace
):
    """Sanity: without the header, the existing immediate-dispatch path
    fires. This guards against accidentally inverting the default."""
    from crum import impersonate

    from pi_dash.db.models import Issue, Project, State
    from pi_dash.prompting.seed import seed_default_template

    seed_default_template()
    with impersonate(workspace.owner):
        project = Project.objects.create(
            name="DispDef", identifier="DD", workspace=workspace,
            created_by=workspace.owner,
        )
        todo = State.objects.create(
            name="Todo", project=project, group="unstarted"
        )
        in_progress = State.objects.create(
            name="In Progress", project=project, group="started"
        )
        issue = Issue.objects.create(
            name="Task", workspace=workspace, project=project,
            state=todo, created_by=workspace.owner,
        )

    resp = session_client.patch(
        f"/api/workspaces/{workspace.slug}/projects/{project.id}/issues/{issue.id}/",
        {"state_id": str(in_progress.id)},
        format="json",
    )
    assert resp.status_code == status.HTTP_204_NO_CONTENT
    assert AgentRun.objects.filter(work_item=issue).count() == 1
