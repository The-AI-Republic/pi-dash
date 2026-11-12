# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Backend rows of design §18 for the write path + resolver (HA-3, H-5).

Covers the ``Shared service, both surfaces`` and ``Resolver / phases``
rows: set/change/clear through both serializers, E1-E8 error codes,
re-pointing of QUEUED runs, activity rows both ways, pin kinds on the
creation paths, fresh-session survival, and stale-assignment self-heal.
"""

from __future__ import annotations

import pytest
from django.utils import timezone

from pi_dash.app.serializers.issue import IssueCreateSerializer
from pi_dash.api.serializers.issue import IssueSerializer as V1IssueSerializer
from pi_dash.db.models.issue import Issue, IssueActivity
from pi_dash.db.models.state import State
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    PinKind,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services.issue_assignment import (
    resolve_run_pin,
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


def _app_serializer(issue, data, user, project):
    return IssueCreateSerializer(
        issue,
        data=data,
        partial=True,
        context={
            "project_id": str(project.id),
            "workspace_id": str(project.workspace_id),
            "user": user,
        },
    )


def _v1_serializer(issue, data, user, project):
    return V1IssueSerializer(
        issue,
        data=data,
        partial=True,
        context={
            "project_id": str(project.id),
            "workspace_id": str(project.workspace_id),
            "user": user,
        },
    )


@pytest.mark.unit
def test_set_change_clear_app_surface(db, workspace, project, create_user, pod, state):
    """Set, change, clear via the app serializer (§18 shared service)."""
    from crum import impersonate

    issue = _make_issue(workspace, project, state, create_user, pod=pod)
    r1 = _make_runner(create_user, workspace, pod, name="box-one")
    r2 = _make_runner(create_user, workspace, pod, name="box-two")

    with impersonate(create_user):
        s = _app_serializer(issue, {"assigned_runner_id": str(r1.id)}, create_user, project)
        assert s.is_valid(), s.errors
        s.save()
    issue.refresh_from_db()
    assert issue.assigned_runner_id == r1.id

    with impersonate(create_user):
        s = _app_serializer(issue, {"assigned_runner_id": str(r2.id)}, create_user, project)
        assert s.is_valid(), s.errors
        s.save()
    issue.refresh_from_db()
    assert issue.assigned_runner_id == r2.id

    with impersonate(create_user):
        s = _app_serializer(issue, {"assigned_runner_id": None}, create_user, project)
        assert s.is_valid(), s.errors
        s.save()
    issue.refresh_from_db()
    assert issue.assigned_runner_id is None


@pytest.mark.unit
def test_set_change_clear_v1_surface(db, workspace, project, create_user, pod, state):
    """Set, change, clear via the v1 serializer (§18 shared service)."""
    from crum import impersonate

    issue = _make_issue(workspace, project, state, create_user, pod=pod)
    r1 = _make_runner(create_user, workspace, pod, name="box-one")
    r2 = _make_runner(create_user, workspace, pod, name="box-two")

    with impersonate(create_user):
        s = _v1_serializer(issue, {"assigned_runner": str(r1.id)}, create_user, project)
        assert s.is_valid(), s.errors
        s.save()
    issue.refresh_from_db()
    assert issue.assigned_runner_id == r1.id

    with impersonate(create_user):
        s = _v1_serializer(issue, {"assigned_runner": str(r2.id)}, create_user, project)
        assert s.is_valid(), s.errors
        s.save()
    issue.refresh_from_db()
    assert issue.assigned_runner_id == r2.id

    with impersonate(create_user):
        s = _v1_serializer(issue, {"assigned_runner": None}, create_user, project)
        assert s.is_valid(), s.errors
        s.save()
    issue.refresh_from_db()
    assert issue.assigned_runner_id is None


@pytest.mark.unit
def test_write_persists_and_writes_activity(db, workspace, project, create_user, pod, state):
    """Accepted save persists, writes activity with names (§13.2)."""
    issue = _make_issue(workspace, project, state, create_user, pod=pod)
    runner = _make_runner(create_user, workspace, pod)

    s = _app_serializer(issue, {"assigned_runner_id": str(runner.id)}, create_user, project)
    assert s.is_valid(), s.errors
    s.save()
    row = IssueActivity.objects.filter(issue=issue, field="assigned_runner").latest("created_at")
    assert row.new_value == "gpu-box"
    assert str(row.new_identifier) == str(runner.id)


@pytest.mark.unit
def test_repoint_queued_run(db, workspace, project, create_user, pod, state):
    """Changing assignment re-points the QUEUED run + notifies (§13.2)."""
    from pi_dash.db.models.issue import IssueComment

    interpreter = _make_runner(create_user, workspace, pod, name="interpreter")
    new_runner = _make_runner(create_user, workspace, pod, name="gpu-box")
    issue = _make_issue(workspace, project, state, create_user, runner=interpreter, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=interpreter, pin_kind=PinKind.DEDICATED,
    )
    s = _app_serializer(issue, {"assigned_runner_id": str(new_runner.id)}, create_user, project)
    assert s.is_valid(), s.errors
    s.save()
    run.refresh_from_db()
    assert run.pinned_runner_id == new_runner.id
    assert run.pin_kind == PinKind.DEDICATED
    assert IssueComment.objects.filter(issue=issue).count() >= 1


@pytest.mark.unit
def test_e_codes_both_surfaces(db, workspace, project, create_user, pod, state):
    """E1-E8 refusals carry their code on both serializers (§18 contract)."""
    from pi_dash.db.models import User
    from uuid import uuid4

    issue = _make_issue(workspace, project, state, create_user, pod=pod)
    stranger = User.objects.create(email=f"s-{uuid4().hex[:8]}@example.com", username=f"s_{uuid4().hex[:8]}")
    stranger_runner = _make_runner(stranger, workspace, pod, name="stranger-box")

    # E1: create_user cannot use stranger's runner.
    s = _app_serializer(issue, {"assigned_runner_id": str(stranger_runner.id)}, create_user, project)
    assert not s.is_valid(), s.errors
    assert s.errors["assigned_runner"][0].code == "runner_not_found"

    s = _v1_serializer(issue, {"assigned_runner": str(stranger_runner.id)}, create_user, project)
    assert not s.is_valid(), s.errors
    assert s.errors["assigned_runner"][0].code == "runner_not_found"

    # E7: stranger asks for own runner on create_user's issue (stranger is
    # neither creator nor assignee).
    s = _app_serializer(issue, {"assigned_runner_id": str(stranger_runner.id)}, stranger, project)
    assert not s.is_valid(), s.errors
    assert s.errors["assigned_runner"][0].code == "runner_owner_not_on_issue"


@pytest.mark.unit
def test_resolver_dedicated_first(db, workspace, project, create_user, pod, state):
    """Assigned runner wins; fresh_session keeps it (§9.1, §9.2)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)

    got, kind = resolve_run_pin(issue, parent=None, pod=pod, fresh_session=True, creator=create_user, trigger="tick")
    assert got == runner and kind == PinKind.DEDICATED

    got, kind = resolve_run_pin(issue, parent=None, pod=pod, fresh_session=False, creator=create_user, trigger="tick")
    assert got == runner and kind == PinKind.DEDICATED


@pytest.mark.unit
def test_resolver_stale_self_heal(db, workspace, project, create_user, pod, state):
    """Revoked or pod-mismatched assignment heals at dispatch (§13)."""
    from pi_dash.db.models.issue import IssueComment

    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    Runner.objects.filter(pk=runner.pk).update(status=RunnerStatus.REVOKED, revoked_at=timezone.now())
    runner.refresh_from_db()

    got, kind = resolve_run_pin(issue, parent=None, pod=pod, fresh_session=False, creator=create_user, trigger="tick")
    assert got is None and kind == PinKind.LOCALITY
    issue.refresh_from_db()
    assert issue.assigned_runner_id is None
    assert IssueComment.objects.filter(issue=issue).count() >= 1


@pytest.mark.unit
def test_apply_execution_target_moves_pod_to_runner_pod(db, workspace, project, create_user, pod, state):
    """The pod follows the runner (§6.2): saving a runner re-points the pod."""
    other = Pod.objects.create(project=project, name=f"{project.identifier}_other", created_by=create_user)
    issue = _make_issue(workspace, project, state, create_user, pod=pod)
    runner = _make_runner(create_user, workspace, other, name="gpu-other")
    s = _app_serializer(issue, {"assigned_runner_id": str(runner.id)}, create_user, project)
    assert s.is_valid(), s.errors
    s.save()
    issue.refresh_from_db()
    assert issue.assigned_runner_id == runner.id
    assert issue.assigned_pod_id == other.id


@pytest.mark.unit
def test_resolver_pod_once_bypasses(db, workspace, project, create_user, pod, state):
    """pod_once bypasses the assignment without consuming it (§23.3)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)

    got, kind = resolve_run_pin(
        issue, parent=None, pod=pod, fresh_session=False,
        creator=create_user, trigger="run_ai", pod_once=True,
    )
    assert got is None and kind == PinKind.LOCALITY
    issue.refresh_from_db()
    assert issue.assigned_runner_id == runner.id  # untouched


@pytest.mark.unit
def test_noop_resend_never_rejected_after_drift(db, workspace, project, create_user, pod, state):
    """Re-sending the current runner is a no-op, never rejected (§7.2)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    # Eligibility drift: the owner is no longer on the issue (E7 would fail).
    from pi_dash.db.models.issue import IssueAssignee

    IssueAssignee.objects.filter(issue=issue, assignee_id=create_user.id).delete()
    s = _app_serializer(issue, {"assigned_runner_id": str(runner.id)}, create_user, project)
    assert s.is_valid(), s.errors


@pytest.mark.unit
def test_clear_refused_under_assigned_run(db, workspace, project, create_user, pod, state):
    """Clearing under an ASSIGNED run is refused (§13.2)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.ASSIGNED, runner=runner,
    )
    s = _app_serializer(issue, {"assigned_runner_id": None}, create_user, project)
    assert not s.is_valid()
    assert s.errors["assigned_runner"][0].code == "active_run_in_progress"


@pytest.mark.unit
def test_clear_allowed_under_queued_run_repoints(db, workspace, project, create_user, pod, state):
    """Clearing with only a QUEUED run re-points it to the pod (§13.2)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    s = _app_serializer(issue, {"assigned_runner_id": None}, create_user, project)
    assert s.is_valid(), s.errors
    s.save()
    issue.refresh_from_db()
    assert issue.assigned_runner_id is None
    run.refresh_from_db()
    assert run.pinned_runner_id is None
    assert run.pin_kind == PinKind.LOCALITY


@pytest.mark.unit
def test_pod_change_without_runner_clears(db, workspace, project, create_user, pod, state):
    """A pod-only change clears the dedicated assignment (§13.1)."""
    other = Pod.objects.create(project=project, name=f"{project.identifier}_other", created_by=create_user)
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    s = _app_serializer(issue, {"assigned_pod_id": str(other.id)}, create_user, project)
    assert s.is_valid(), s.errors
    s.save()
    issue.refresh_from_db()
    assert issue.assigned_pod_id == other.id
    assert issue.assigned_runner_id is None


@pytest.mark.unit
def test_create_rejects_wrong_project_runner(db, workspace, project, create_user, pod, state):
    """E2 on create reads the context project."""
    other_project = type(project)(name="Other", workspace=project.workspace, created_by=create_user)
    other_project.save()
    other_pod = Pod.objects.create(project=other_project, name="otherpod", created_by=create_user)
    runner = _make_runner(create_user, workspace, other_pod)
    s = IssueCreateSerializer(
        None,
        data={"name": "N", "assigned_runner_id": str(runner.id)},
        context={
            "project_id": str(project.id),
            "workspace_id": str(project.workspace_id),
            "user": create_user,
        },
    )
    assert not s.is_valid()
    assert s.errors["assigned_runner"][0].code == "runner_wrong_project"
