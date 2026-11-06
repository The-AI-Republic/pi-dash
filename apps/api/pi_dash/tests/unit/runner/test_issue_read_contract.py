# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Read-side contract for the dedicated runner (design §7.4, §14.1-§14.3, §14.7).

Covers: ``assigned_runner_id`` + ``assigned_runner_detail`` on app list/detail
and v1, ``dedicated_runner_lost_action`` writable on project serializers,
``assignable_for`` row fields, ``dispatch_wait`` with ``pause_reason`` /
``expires_at``, and pod/runner activity trackers. Write validation lands with
dispatch (H-5), not here.
"""

from __future__ import annotations

import pytest
from django.utils import timezone
from rest_framework import status

from pi_dash.app.serializers.issue import IssueDetailSerializer
from pi_dash.api.serializers.issue import IssueSerializer as V1IssueSerializer
from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import State
from pi_dash.runner.models import AgentRun, AgentRunStatus, PinKind, Pod, Runner, RunnerStatus
from pi_dash.runner.services.issue_assignment import (
    assignable_for_runner,
    assigned_runner_detail,
    build_dispatch_wait,
)


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def state(workspace, project):
    return State.objects.create(
        name="Backlog", group="backlog", workspace=workspace, project=project,
        sequence=1.0, default=True,
    )


def _make_runner(user, workspace, pod, name="gpu-box"):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _make_issue(workspace, project, state, user, *, runner=None, pod=None):
    # ``BaseModel.save`` stamps ``created_by`` from the crum current user,
    # clobbering an explicit ``created_by=`` kwarg when anonymous; the pod
    # tests use the same impersonate pattern.
    from crum import impersonate

    with impersonate(user):
        return Issue.objects.create(
            name="An issue",
            project=project,
            workspace=workspace,
            state=state,
            created_by=user,
            assigned_pod=pod,
            assigned_runner=runner,
        )


@pytest.mark.unit
def test_issue_list_and_detail_read_dedicated_fields(db, workspace, project, create_user, pod, state):
    """List carries the id; detail carries id + detail object (design §14.1)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)

    from pi_dash.app.serializers.issue import IssueSerializer

    listed = IssueSerializer(issue).data
    assert str(listed["assigned_runner_id"]) == str(runner.id)

    detail = IssueDetailSerializer(issue, context={}).data
    assert str(detail["assigned_runner_id"]) == str(runner.id)
    d = detail["assigned_runner_detail"]
    assert d["id"] == str(runner.id)
    assert d["name"] == "gpu-box"
    assert d["status"] == "online"
    assert d["pod"] == str(pod.id)
    assert d["owner"] == str(create_user.id)
    assert d["host_label"] == ""
    assert d["is_busy"] is False
    assert d["queue_depth"] == 0


@pytest.mark.unit
def test_pod_only_issue_reads_null_dedicated_fields(db, workspace, project, create_user, pod, state):
    """A pod-only issue serializes as before plus two null fields (§18)."""
    issue = _make_issue(workspace, project, state, create_user, pod=pod)

    from pi_dash.app.serializers.issue import IssueSerializer

    listed = IssueSerializer(issue).data
    assert listed["assigned_runner_id"] is None
    detail = IssueDetailSerializer(issue, context={}).data
    assert detail["assigned_runner_id"] is None
    assert detail["assigned_runner_detail"] is None


@pytest.mark.unit
def test_v1_reads_dedicated_fields(db, workspace, project, create_user, pod, state):
    """v1 mirrors the app read with the bare ``assigned_runner`` name (§14.2)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    data = V1IssueSerializer(issue).data
    assert str(data["assigned_runner"]) == str(runner.id)
    assert data["assigned_runner_detail"]["name"] == "gpu-box"


@pytest.mark.unit
def test_project_lost_action_writable_both_surfaces(db, project):
    """§6.6: the lost-runner setting is writable on both project serializers."""
    from pi_dash.app.serializers.project import ProjectDetailSerializer
    from pi_dash.api.serializers.project import ProjectCreateSerializer

    assert ProjectDetailSerializer(project).data["dedicated_runner_lost_action"] == "fail_to_backlog"

    serializer = ProjectCreateSerializer(
        data={"name": "P", "identifier": "P1", "dedicated_runner_lost_action": "fall_back"},
        context={"workspace_id": str(project.workspace_id)},
    )
    assert serializer.is_valid(), serializer.errors
    assert serializer.validated_data["dedicated_runner_lost_action"] == "fall_back"


@pytest.mark.unit
def test_assignable_for_evaluation(db, workspace, project, create_user, pod, state):
    """E1/E7/E4 ordering for the list read (design §7.1 read side)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)

    ok, code = assignable_for_runner(runner, user=create_user, issue=issue)
    assert ok and code == ""

    # A runner the requester cannot use reads runner_not_found (E1).
    from pi_dash.db.models import User
    from uuid import uuid4

    other = User.objects.create(email=f"o-{uuid4().hex[:8]}@example.com", username=f"o_{uuid4().hex[:8]}")
    ok, code = assignable_for_runner(runner, user=other, issue=issue)
    assert not ok and code == "runner_not_found"

    # Owner is not on the issue (E7): other owns the runner, but is not
    # creator/assignee. E1 passes (other owns it), E2 passes (same project),
    # E7 fails.
    other_runner = _make_runner(other, workspace, pod, name="other-box")
    own_issue = _make_issue(workspace, project, state, create_user)
    ok, code = assignable_for_runner(other_runner, user=create_user, issue=own_issue)
    assert not ok and code == "runner_not_found"  # E1 first: create_user cannot use it


@pytest.mark.unit
def test_runner_list_assignable_for_rows(db, session_client, workspace, project, create_user, pod, state):
    """``GET /api/runners/?assignable_for=`` rows carry the four fields (§7.4)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)

    resp = session_client.get(
        "/api/runners/",
        {"workspace": str(workspace.id), "project": str(project.id),
         "assignable_for": str(issue.id)},
    )
    assert resp.status_code == status.HTTP_200_OK
    row = next(r for r in resp.data if str(r["id"]) == str(runner.id))
    assert row["assignable"] is True
    assert row["not_assignable_code"] == ""
    assert row["is_busy"] is False
    assert row["queue_depth"] == 0

    # Without the parameter the keys stay absent (pre-§7.4 shape).
    plain = session_client.get(
        "/api/runners/",
        {"workspace": str(workspace.id), "project": str(project.id)},
    )
    assert plain.status_code == status.HTTP_200_OK
    plain_row = next(r for r in plain.data if str(r["id"]) == str(runner.id))
    assert "assignable" not in plain_row


@pytest.mark.unit
def test_dispatch_wait_offline_dedicated(db, workspace, project, create_user, pod, state):
    """A dedicated run on an offline runner reports runner_offline + expiry (§14.3)."""
    runner = _make_runner(create_user, workspace, pod)
    runner.status = RunnerStatus.OFFLINE
    runner.last_heartbeat_at = None
    runner.save(update_fields=["status", "last_heartbeat_at"])
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    wait = build_dispatch_wait(run, viewer=create_user)
    assert wait["reason"] == "runner_offline"
    assert wait["pin_kind"] == "dedicated"
    assert wait["runner"] == str(runner.id)
    assert wait["runner_name"] == "gpu-box"
    assert wait["pause_reason"] is None
    assert wait["expires_at"] is not None  # queued_at + 12h bound
    assert wait["releasable"] is False  # H-13


@pytest.mark.unit
def test_dispatch_wait_paused_wins_and_carries_reason(db, workspace, project, create_user, pod, state):
    """Paused wins over offline and carries ``pause_reason`` (§11.1, §23.5)."""
    runner = _make_runner(create_user, workspace, pod)
    runner.status = RunnerStatus.OFFLINE
    runner.paused_at = timezone.now()
    runner.save(update_fields=["status", "paused_at"])
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.LOCALITY,
    )
    wait = build_dispatch_wait(run, viewer=create_user)
    assert wait["reason"] == "runner_paused"
    assert wait["pause_reason"] == "dispatch_paused"
    assert wait["expires_at"] is None  # locality pin has no bound


@pytest.mark.unit
def test_run_serializer_carries_pin_fields(db, workspace, project, create_user, pod, state):
    """``AgentRunSerializer`` carries pin fields + dispatch_wait (§14.3)."""
    from pi_dash.runner.serializers import AgentRunSerializer

    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    data = AgentRunSerializer(run, context={"request": None}).data
    assert str(data["pinned_runner"]) == str(runner.id)
    assert data["pinned_runner_name"] == "gpu-box"
    assert data["pin_kind"] == "dedicated"
    assert data["dispatch_wait"] is not None


@pytest.mark.unit
def test_activity_trackers_names(db, workspace, project, create_user, pod, state):
    """Pod + runner changes record names as old/new values (§14.7)."""
    from pi_dash.bgtasks.issue_activities_task import track_assigned_runner

    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    from django.utils import timezone as tz

    activities = []
    track_assigned_runner(
        requested_data={"assigned_runner_id": None},
        current_instance={"assigned_runner_id": str(runner.id)},
        issue_id=issue.id, project_id=project.id, workspace_id=workspace.id,
        actor_id=create_user.id, issue_activities=activities, epoch=1,
    )
    assert len(activities) == 1
    entry = activities[0]
    assert entry.field == "assigned_runner"
    assert entry.old_value == "gpu-box"
    assert entry.new_value == ""
